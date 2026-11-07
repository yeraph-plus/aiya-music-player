//! 推流频道控制层 v2：多频道 + 双控制形态。
//!
//! 应用层只有两个接触面——配置渲染（[`render_liq`] 纯函数）与运行时控制
//! （telnet 短连 + now.jsonl 回灌轮询）。频道定义来自 DB
//! （`aiya_data::station`；N1.0 的 env 测试通道已退役）。
//!
//! 控制形态（boot 时定，`AIYA_LIQ_HOST` 有无）：
//!   - **InProcess**（未置）：server 直接 spawn liquidsoap 子进程（单容器形
//!     态；守护 = wait 循环 + 指数退避，stop 信号走 mpsc）。
//!   - **Sidecar**（置位，compose 双容器）：渲染结果写共享卷
//!     `<hls_root>/conf/{ch}/station.liq` + `ON` 开关位；liq 容器内的
//!     keeper.sh（docker/liq-keeper.sh）对账 spawn/kill；server 只写配置
//!     不 spawn，telnet 走 `AIYA_LIQ_HOST:port`。
//!
//! 渲染按 liq 2.4.6（官方镜像实测口径）。路径参数全部用 **liq 容器视角**
//! （Sidecar 下共享卷在 liq 侧挂 /hls）——server 写文件与读回灌走 server
//! 视角（hls_root），两套视角由挂载约定对齐（同卷）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxPath, State};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc as StdArc;

use crate::state::ServerState;

/* ---- 渲染（纯函数，单测锁定关键指令行） ---- */

pub struct LiqConfig {
    /// 频道段目录（**liq 视角**绝对路径——Sidecar 下是 /hls/{dir}）
    pub liq_dir: String,
    /// 垫底歌单（liq 视角）
    pub liq_m3u: String,
    pub telnet_port: u16,
    /// telnet 绑定地址：InProcess 127.0.0.1；Sidecar 0.0.0.0（compose 内网）
    pub telnet_bind: &'static str,
}

/// 渲染单频道 .liq（2.4.6 语法：cross 位置 duration、directory 标签、
/// %audio/aac、方法式 on_track（2.1/2.4 同构）。
pub fn render_liq(c: &LiqConfig) -> String {
    format!(
        r##"settings.init.allow_root.set(true)
set("log.stdout", false)
set("log.file.path", "{dir}/liq.log")
set("log.file.append", true)
set("server.telnet", true)
set("server.telnet.bind_addr", "{bind}")
set("server.telnet.port", {port})

# 垫底歌单（randomize）→ request.queue 插播在 fallback 链顶
base = playlist(id="base", mode="randomize", "{m3u}")
req = request.queue(id="request")
src = fallback(track_sensitive=false, [req, crossfade(duration=2., base), blank(duration=2.)])

# 回灌：每轨开始覆盖写 now.jsonl（宿主 1s 轮询进 SSE）。不用 stdout print——
# 管道块缓冲攒行不刷；file.write 每轨落盘一次，"当前轨"语义免掉轮转 GC。
# 元数据键缺失时 liq 给空串（?? 只救 null），判空兜底交给下面的 if。
def now_write(m) =
  t = m["title"] ?? "unknown"
  file.write(data=(if t == "" then "unknown" else t end) ^ "\n", append=false, "{dir}/now.jsonl")
end
# 钩子是副作用方法（返回 unit）——独立调用，源继续走 fallback 链
src.on_track(synchronous=true, now_write)

output.file.hls(playlist="live.m3u8", segment_duration=6.0, segments=6, segments_overhead=6, fallible=true, "{dir}", [("stream", %mp3)], src)
"##,
        dir = c.liq_dir,
        bind = c.telnet_bind,
        port = c.telnet_port,
        m3u = c.liq_m3u,
    )
}

/* ---- 运行态 ---- */

#[derive(Clone, Copy, PartialEq, Debug)]
enum ControlMode {
    InProcess,
    Sidecar,
}

pub struct StationCtl {
    inner: Arc<Inner>,
}

struct Inner {
    channels: Mutex<HashMap<String, ChannelRun>>,
    hls_root: OnceLock<PathBuf>,
    mode: OnceLock<ControlMode>,
    liq_host: OnceLock<String>,
    /// telnet 端口分配记录（12701 起线性探测）
    ports: Mutex<Vec<u16>>,
}

struct ChannelRun {
    dir: String,
    port: u16,
    /// 频道段目录（server 视角；回灌轮询锚点）
    hls_dir: PathBuf,
    visibility: String,
    running: Arc<AtomicBool>,
    last_track: Arc<Mutex<Option<String>>>,
    /// InProcess 守护任务的停播信号；Sidecar = None（摘 ON 位即可）
    stop_tx: Mutex<Option<tokio::sync::mpsc::Sender<()>>>,
}

impl StationCtl {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                channels: Mutex::new(HashMap::new()),
                hls_root: OnceLock::new(),
                mode: OnceLock::new(),
                liq_host: OnceLock::new(),
                ports: Mutex::new(Vec::new()),
            }),
        }
    }

    fn alloc_port(&self) -> u16 {
        let mut used = self
            .inner
            .ports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut p = 12701u16;
        while used.contains(&p) {
            p += 1;
        }
        used.push(p);
        p
    }

    fn free_port(&self, p: u16) {
        self.inner
            .ports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|x| *x != p);
    }

    /// station_list 的运行时字段组装（running/nowPlaying 按 id 查表）。
    pub fn runtime_of(&self, station_id: &str) -> (bool, Option<String>) {
        // guard 只护取值：块内取满快照（目录/标志/回灌），文件判断放块外。
        // last_track 的 MutexGuard 临时必须独立 let——放元组尾表达式会活到
        // 块尾，借用链跨过外层 guard 的 drop（E0597）。
        let (hls_dir, atomic_running, last_track) = {
            let map = self
                .inner
                .channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(c) = map.get(station_id) else {
                return (false, None);
            };
            let hls_dir = c.hls_dir.clone();
            let atomic_running = c.running.load(Ordering::SeqCst);
            let last_track = c
                .last_track
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            (hls_dir, atomic_running, last_track)
        };
        // Sidecar 的 running 判据 = keeper 运行位（顶层 conf/{dir}/ 下的
        // ON + .pid），≤2s 对账误差；InProcess 用守护任务维护的原子位。
        let running = match (self.inner.mode.get(), self.inner.hls_root.get()) {
            // join 只认相对名：hls_dir 是绝对路径（/hls/{dir}），join 会整体
            // 替换根——conf 顶层目录用 dir 相对名拼（PathBuf::join 陷阱）
            (Some(ControlMode::Sidecar), Some(root)) => {
                let dir = hls_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let conf = root.join("conf").join(&dir);
                conf.join("ON").is_file() && conf.join(".pid").is_file()
            }
            _ => atomic_running,
        };
        (running, last_track)
    }

    /// /hls 挂载根（station 启用时优先于 AIYA_HLS_DIR 试验位）。
    pub fn hls_root(&self) -> Option<PathBuf> {
        self.inner.hls_root.get().cloned()
    }

    /// 可见性表（public 路径豁免集重建的数据源，N1.3）。
    pub fn public_dirs(&self) -> Vec<String> {
        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|c| c.visibility == aiya_data::station::VISIBILITY_PUBLIC)
            .map(|c| c.dir.clone())
            .collect()
    }

    /// 手动切歌。
    pub async fn skip(&self, station_id: &str) -> Result<Value, String> {
        let port = self.port_of(station_id)?;
        let out = telnet_cmd(&self.liq_host_of(), port, "request.skip").await?;
        Ok(json!({ "raw": out.trim().to_string() }))
    }

    /// 点歌插播：本地文件路径走 file 协议。
    pub async fn push_file(&self, station_id: &str, path: &str) -> Result<Value, String> {
        let port = self.port_of(station_id)?;
        let uri = format!("file://{}", path.replace('\\', "/"));
        let out = telnet_cmd(&self.liq_host_of(), port, &format!("request.push {uri}")).await?;
        Ok(json!({ "raw": out.trim().to_string() }))
    }

    fn port_of(&self, station_id: &str) -> Result<u16, String> {
        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(station_id)
            .map(|c| c.port)
            .ok_or_else(|| format!("station not running: {station_id}"))
    }

    /// Sidecar 的 ON 开关位路径（顶层 conf/{dir}/ON）。
    fn sidecar_conf_on(inner: &Inner, station_id: &str) -> PathBuf {
        let dir = inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(station_id)
            .map(|c| c.dir.clone())
            .unwrap_or_default();
        inner
            .hls_root
            .get()
            .map(|r| r.join("conf").join(&dir).join("ON"))
            .unwrap_or_else(|| PathBuf::from(format!("/hls/conf/{dir}/ON")))
    }

    fn liq_host_of(&self) -> String {
        self.inner
            .liq_host
            .get()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".into())
    }

    /// 开播（station_activate on / boot 批量）：渲染 + 起管。
    pub async fn start(
        &self,
        st: &Arc<ServerState>,
        row: &aiya_data::station::StationRow,
    ) -> Result<(), String> {
        let mode = *self.inner.mode.get().ok_or("station not booted")?;
        let hls_root = self
            .inner
            .hls_root
            .get()
            .ok_or("station not booted")?
            .clone();
        let hls_dir = hls_root.join(&row.hls_dir);
        std::fs::create_dir_all(&hls_dir).map_err(|e| format!("create hls dir: {e}"))?;

        let port = self.alloc_port();
        let telnet_bind: &'static str = if mode == ControlMode::Sidecar {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        // liq 视角路径：Sidecar 下共享卷挂 liq 容器 /hls；InProcess 即本地路径
        let liq_prefix = if mode == ControlMode::Sidecar {
            format!("/hls/{}", row.hls_dir)
        } else {
            hls_dir.to_string_lossy().replace('\\', "/")
        };

        // 垫底歌单：全库在线曲目随机 100（N1.3 起按 station_playlists/防重复细化）
        let entries: Vec<(String, String)> = crate::state::db_lock(st)
            .send(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT path, COALESCE(title, '') FROM tracks WHERE availability = 'online'
                     ORDER BY RANDOM() LIMIT 100",
                )?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                let mut v = Vec::new();
                for r in rows {
                    v.push(r?);
                }
                Ok(v)
            })
            .map_err(|e| e.to_string())?;
        if entries.is_empty() {
            self.free_port(port);
            return Err(format!("曲库无在线曲目——频道 {} 未启动", row.name));
        }
        let m3u_path = hls_dir.join("default.m3u");
        // annotate 行：title 内嵌双引号会截断 liq 的 annotate 解析，换掉；
        // 空标题（无标签文件）退回裸路径——回灌的 unknown 占位由模板兜
        let m3u_text = entries
            .iter()
            .map(|(path, title)| {
                let safe = title.replace('"', "'");
                if safe.is_empty() {
                    path.clone()
                } else {
                    format!("annotate:title=\"{safe}\":{path}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&m3u_path, m3u_text).map_err(|e| format!("write m3u: {e}"))?;

        let text = render_liq(&LiqConfig {
            liq_dir: liq_prefix.clone(),
            liq_m3u: format!("{liq_prefix}/default.m3u"),
            telnet_port: port,
            telnet_bind,
        });

        let run = ChannelRun {
            dir: row.hls_dir.clone(),
            port,
            hls_dir: hls_dir.clone(),
            visibility: row.visibility.clone(),
            running: Arc::new(AtomicBool::new(false)),
            last_track: Arc::new(Mutex::new(None)),
            stop_tx: Mutex::new(None),
        };

        match mode {
            ControlMode::InProcess => {
                let liq_path = hls_dir.join("station.liq");
                std::fs::write(&liq_path, &text).map_err(|e| format!("write liq: {e}"))?;
                // --check 闸（本机有 liq 才有意义；失败信封带 stderr 末 20 行）
                let out = tokio::process::Command::new("liquidsoap")
                    .arg("--check")
                    .arg(&liq_path)
                    .output()
                    .await
                    .map_err(|e| format!("run liquidsoap --check: {e}"))?;
                if !out.status.success() {
                    self.free_port(port);
                    let tail: String = String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .rev()
                        .take(20)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Err(format!("liq check failed:\n{tail}"));
                }
                let (stop_tx, stop_rx) = tokio::sync::mpsc::channel::<()>(1);
                *run.stop_tx
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(stop_tx);
                self.spawn_guard(liq_path, Arc::clone(&run.running), stop_rx);
            }
            ControlMode::Sidecar => {
                // 配置卷：顶层 conf/{ch}/station.liq + ON 位（keeper 契约：
                // 扫 /hls/conf/*/station.liq——与频道段目录 /hls/{ch}/ 平行）
                let conf = hls_root.join("conf").join(&row.hls_dir);
                std::fs::create_dir_all(&conf).map_err(|e| format!("create conf: {e}"))?;
                let conf_liq = conf.join("station.liq");
                std::fs::write(&conf_liq, &text).map_err(|e| format!("write liq: {e}"))?;
                let manifest = conf.join("manifest.json");
                std::fs::write(
                    &manifest,
                    json!({
                        "stationId": row.id,
                        "dir": row.hls_dir,
                        "port": port,
                    })
                    .to_string(),
                )
                .map_err(|e| format!("write manifest: {e}"))?;
                std::fs::write(conf.join("ON"), b"1").map_err(|e| format!("write ON: {e}"))?;
            }
        }

        // 回灌轮询（两种形态同源：now.jsonl 由 liq 的 on_track 覆盖写）
        let poll_last = Arc::clone(&run.last_track);
        let poll_bus = st.events.clone();
        let poll_dir = hls_dir.clone();
        tokio::spawn(async move {
            let now_path = poll_dir.join("now.jsonl");
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                let Ok(text) = tokio::fs::read_to_string(&now_path).await else {
                    continue;
                };
                let t = text.trim();
                if t.is_empty() {
                    continue;
                }
                let changed = {
                    let mut g = poll_last
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if g.as_deref() == Some(t) {
                        false
                    } else {
                        *g = Some(t.to_string());
                        true
                    }
                };
                if changed {
                    poll_bus.emit("stations", &json!({ "nowPlaying": t }));
                }
            }
        });

        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(row.id.clone(), run);
        Ok(())
    }

    /// 停播（station_activate off）：InProcess 发 stop 信号；Sidecar 摘 ON 位
    /// （keeper 2s 内 kill）。段目录与配置保留——再开播热恢复。
    pub fn stop(&self, station_id: &str) -> Result<(), String> {
        let mode = *self.inner.mode.get().ok_or("station not booted")?;
        let (port, stop_tx, _sidecar_dir) = {
            let map = self
                .inner
                .channels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let c = map
                .get(station_id)
                .ok_or_else(|| format!("station not running: {station_id}"))?;
            let port = c.port;
            let stop_tx = c
                .stop_tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let dir = c.hls_dir.clone();
            (port, stop_tx, dir)
        };
        match mode {
            ControlMode::InProcess => {
                if let Some(tx) = stop_tx {
                    std::mem::drop(tx.send(()));
                }
            }
            ControlMode::Sidecar => {
                let on = Self::sidecar_conf_on(&self.inner, station_id);
                match std::fs::remove_file(&on) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("rm ON: {e}")),
                }
            }
        }
        self.free_port(port);
        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(station_id);
        Ok(())
    }

    /// InProcess 守护：wait 循环 + 指数退避；stop 信号到达即 kill 子进程退出。
    fn spawn_guard(
        &self,
        liq_path: PathBuf,
        running: Arc<AtomicBool>,
        mut stop_rx: tokio::sync::mpsc::Receiver<()>,
    ) {
        tokio::spawn(async move {
            let mut backoff: u64 = 1;
            loop {
                let child = tokio::process::Command::new("liquidsoap")
                    .arg(&liq_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = %e, "station spawn failed, giving up");
                        running.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                running.store(true, Ordering::SeqCst);
                let reason = tokio::select! {
                    r = child.wait() => r.map(|_| ()).map_err(|e| e.to_string()),
                    _ = stop_rx.recv() => Err("stop".into()),
                };
                let _ = child.kill().await;
                running.store(false, Ordering::SeqCst);
                if matches!(reason, Err(ref e) if e == "stop") {
                    return;
                }
                tracing::warn!(backoff_s = backoff, "station process exited, restarting");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        });
    }
}

/// telnet 短连：发一条命令 + quit，读响应到静默窗口（800ms 无新字节）。
async fn telnet_cmd(host: &str, port: u16, cmd: &str) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::net::TcpStream::connect((host, port)).await
    })
    .await
    .map_err(|_| "telnet connect timeout".to_string())?
    .map_err(|e| format!("telnet connect {host}:{port}: {e}"))?;
    s.write_all(format!("{cmd}\nquit\n").as_bytes())
        .await
        .map_err(|e| format!("telnet write: {e}"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match tokio::time::timeout(Duration::from_millis(800), s.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/* ---- boot ---- */

/// 启动钩子：定形态/段根，把 DB 里 active=1 的频道逐个起起来。
/// 失败只记日志不拦启动——推流是附属面，不能让频道问题挡住曲库服务。
pub async fn boot(st: &Arc<ServerState>, data_root: &Path) -> Result<(), String> {
    let hls_root = std::env::var("AIYA_HLS_ROOT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root.join("hls"));
    let liq_host = std::env::var("AIYA_LIQ_HOST")
        .ok()
        .filter(|s| !s.is_empty());
    let mode = if liq_host.is_some() {
        ControlMode::Sidecar
    } else {
        ControlMode::InProcess
    };

    let _ = st.station.inner.hls_root.set(hls_root.clone());
    let _ = st.station.inner.mode.set(mode);
    if let Some(h) = liq_host {
        let _ = st.station.inner.liq_host.set(h);
    }
    std::fs::create_dir_all(&hls_root).map_err(|e| format!("create hls root: {e}"))?;

    let active: Vec<aiya_data::station::StationRow> = crate::state::db_lock(st)
        .send(|conn| aiya_data::station::list_stations(conn))
        .map_err(|e| e.to_string())?;
    // GC：段目录无对应 station 行 = 删除残留（频道删除/异常退出）——
    // boot 时机单点执行，不用常驻任务
    let known: std::collections::HashSet<String> = crate::state::db_lock(st)
        .send(|conn| aiya_data::station::list_stations(conn))
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.hls_dir)
        .collect();
    if let Ok(entries) = std::fs::read_dir(&hls_root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !known.contains(&name) && name.starts_with("ch") {
                let p = e.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                    tracing::info!(dir = %name, "gc: removed orphan station dir");
                }
            }
        }
    }

    let active: Vec<_> = active.into_iter().filter(|r| r.active).collect();
    if active.is_empty() {
        return Ok(());
    }
    tracing::info!(stations = active.len(), mode = ?mode, "station boot");
    for row in active {
        if let Err(e) = st.station.start(st, &row).await {
            tracing::error!(station = %row.name, error = %e, "station start failed");
        }
    }
    Ok(())
}

/* ---- ws 接触点（§12.1）：与 m3u8 同路径空间的对外元数据协议 ---- */

impl StationCtl {
    /// dir → 频道行（ws hello 的站单字段源）。
    fn station_by_dir(
        &self,
        st: &StdArc<ServerState>,
        dir: &str,
    ) -> Option<aiya_data::station::StationRow> {
        crate::state::db_lock(st)
            .send(|conn| aiya_data::station::list_stations(conn))
            .ok()?
            .into_iter()
            .find(|r| r.hls_dir == dir)
    }

    /// `GET /hls/{dir}/ws`：升级后先发 hello（站单静态字段 + 当前正在播），
    /// 之后把 events 总线的 `stations` 通道原样转发（与 SSE 同一状态源）。
    /// 鉴权由 auth 中间件统一处理（public 豁免 / private `?token=`）。
    pub async fn ws_endpoint(
        st: State<StdArc<ServerState>>,
        AxPath(dir): AxPath<String>,
        ws: WebSocketUpgrade,
    ) -> Response {
        let Some(row) = st.station.station_by_dir(&st.0, &dir) else {
            return (axum::http::StatusCode::NOT_FOUND, "unknown station").into_response();
        };
        ws.on_upgrade(move |socket| async move {
            Self::ws_serve(st.0, row, socket).await;
        })
    }

    async fn ws_serve(
        st: StdArc<ServerState>,
        row: aiya_data::station::StationRow,
        socket: WebSocket,
    ) {
        let (mut sender, mut receiver) = socket.split();
        let (running, now_playing) = st.station.runtime_of(&row.id);
        let hello = json!({
            "type": "hello",
            "id": row.id,
            "name": row.name,
            "subtitle": row.subtitle,
            "description": row.description,
            "cover": row.cover_url,
            "hlsUrl": format!("/hls/{}/live.m3u8", row.hls_dir),
            "visibility": row.visibility,
            "running": running,
            "nowPlaying": now_playing,
        });
        if sender
            .send(Message::Text(hello.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
        let mut rx = st.events.subscribe();
        loop {
            tokio::select! {
                ev = rx.recv() => {
                    match ev {
                        Ok((channel, data)) if channel == "stations" => {
                            if sender
                                .send(Message::Text(format!("{{\"type\":\"track\",\"nowPlaying\":{data}}}").into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => return,
                    }
                }
                // 客户端帧一律丢弃——只借 read 探测断连
                msg = receiver.next() => {
                    if matches!(msg, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(bind: &'static str) -> LiqConfig {
        LiqConfig {
            liq_dir: "/hls/ch1".into(),
            liq_m3u: "/hls/ch1/default.m3u".into(),
            telnet_port: 12701,
            telnet_bind: bind,
        }
    }

    #[test]
    fn liq_template_2_4_6_key_lines() {
        let t = render_liq(&cfg("0.0.0.0"));
        // 2.4.6 形态（§13 逐条实测；%audio/aac 老语法缺绑定同 2.1.3）
        assert!(
            t.contains("crossfade(duration=2., base)"),
            "stdlib crossfade 封装（2.1/2.4 同构；crossfade 首参恒 source）：{t}"
        );
        assert!(
            t.contains("\"/hls/ch1\", [("),
            "目录位置参（2.1/2.4 同构，无 directory 标签）"
        );
        // raw 定界符小心："#STREAM" 里的 `"#` 会提前闭合 r#……，分开断
        assert!(t.contains(r#"%mp3"#), "aac 走 2.4 ffmpeg 复合编码器");
        assert!(
            t.contains("\"stream\""),
            "hls 格式名（避开 # 段名——URL fragment 语义冲突）"
        );
        assert!(
            t.contains("src.on_track(synchronous=true, now_write)"),
            "方法式钩子（2.4.6 stdlib 惯用：synchronous 命名参）"
        );
        assert!(
            t.contains(r#"set("server.telnet.bind_addr", "0.0.0.0")"#),
            "Sidecar 绑定"
        );
        assert!(
            t.contains("settings.init.allow_root.set(true)"),
            "root 闸保留"
        );
        assert!(
            t.contains(r#"file.write(data=(if t == "" then "unknown" else t end)"#),
            "回灌覆盖写"
        );
        assert!(t.contains("/hls/ch1/default.m3u"), "歌单路径");
        // InProcess 形态
        let t2 = render_liq(&cfg("127.0.0.1"));
        assert!(t2.contains(r#"set("server.telnet.bind_addr", "127.0.0.1")"#));
    }
}
