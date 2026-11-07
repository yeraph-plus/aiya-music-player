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
    pub telnet_port: u16,
    /// telnet 绑定地址：InProcess 127.0.0.1；Sidecar 0.0.0.0（compose 内网）
    pub telnet_bind: &'static str,
    /// 频道歌单源定义块（"pl0 = playlist(...)" 每行一条；空 = 无歌单）
    pub program_sources: String,
    /// 未排时段的默认源表达式（"pl0" 或 "base"）
    pub base_expr: String,
    /// 节目表 switch 块（"sch = switch(...)"；空 = 无节目表，走 base 直连）
    pub schedule_block: String,
}

/// m3u 行：annotate 协议注入 title；空标题退裸路径（占位 unknown 由模板兜）
fn annotate_lines(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(path, title)| {
            // 双引号换写、反斜杠剔除（会转义闭合引号）、控制字符抹平——
            // annotate 行按单行解析（审查 P2-3）
            let safe: String = title
                .replace('\\', "")
                .replace('"', "'")
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            if safe.is_empty() {
                path.clone()
            } else {
                format!("annotate:title=\"{safe}\":{path}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 渲染单频道 .liq（2.4.6 实测形态：stdlib crossfade(duration=)、方法式
/// `src.on_track(synchronous=true, f)`、%mp3 编码——%audio/aac 老语法官方
/// 镜像同样缺，%ffmpeg ts 段卡视频流；审查后统一退 %mp3）。
pub fn render_liq(c: &LiqConfig) -> String {
    // 中段源：有节目表 → sch（switch 按时段切歌单）；无 → base 直连。
    // 两条链统一过 crossfade（相邻轨动态过渡，运行时原语）。
    let mid = if c.schedule_block.is_empty() {
        c.base_expr.clone()
    } else {
        "sch".to_string()
    };
    format!(
        r##"settings.init.allow_root.set(true)
set("log.stdout", false)
set("log.file.path", "{dir}/liq.log")
set("log.file.append", true)
set("server.telnet", true)
set("server.telnet.bind_addr", "{bind}")
set("server.telnet.port", {port})

{program_sources}# 未排时段的默认源
base = {base_expr}
req = request.queue(id="request")
{schedule_block}
src = fallback(track_sensitive=false, [req, crossfade(duration=2., {mid}), blank(duration=2.)])

# 回灌：每轨开始覆盖写 now.jsonl（宿主 1s 轮询进 SSE）。不用 stdout print——
# 管道块缓冲攒行不刷；file.write 每轨落盘一次，"当前轨"语义免掉轮转 GC。
# 元数据键缺失时 liq 给空串（?? 只救 null），判空兜底交给下面的 if。
def now_write(m) =
  t = m["title"] ?? "unknown"
  file.write(data=(if t == "" then "unknown" else t end) ^ "\n", append=false, "{dir}/now.jsonl")
end
# 方法返回 unit（副作用调用）：独立执行不重赋值——赋值会把 unit
# 塞进 src 让 fallback 列表炸（2.4.6 实测）
src.on_track(synchronous=true, now_write)

output.file.hls(playlist="live.m3u8", segment_duration=6.0, segments=6, segments_overhead=6, fallible=true, "{dir}", [("stream", %mp3)], src)
"##,
        dir = c.liq_dir,
        bind = c.telnet_bind,
        port = c.telnet_port,
        program_sources = c.program_sources,
        base_expr = c.base_expr,
        schedule_block = c.schedule_block,
        mid = mid,
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
    /// 回灌轮询任务句柄：stop/覆盖前 abort（审查 P1-2，防任务永生泄漏）
    poll_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
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

    /// 可见性热更新（station_update 路径）：内存频道表的快照同步——
    /// public→private 立即回收豁免，private→public 立即放行（审查 P0-2）
    pub fn set_visibility(&self, station_id: &str, visibility: &str) {
        if let Some(c) = self
            .inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(station_id)
        {
            c.visibility = visibility.to_string();
        }
    }

    /// station_list 的运行时字段组装（running/nowPlaying 按 id 查表）。
    pub fn runtime_of(&self, station_id: &str) -> (bool, Option<String>) {
        // guard 只护取值：块内取满快照，文件判断放块外。
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
        // join 只认相对名：hls_dir 是绝对路径，join 会整体替换根。
        let running = match (self.inner.mode.get(), self.inner.hls_root.get()) {
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
        // telnet 是行协议：路径里的 \r\n（Linux 合法文件名）会注入额外命令
        // 行，发送前剥离（审查 P2-4）
        let clean: String = path.chars().filter(|c| !c.is_control()).collect();
        let uri = format!("file://{}", clean.replace('\\', "/"));
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

    fn liq_host_of(&self) -> String {
        self.inner
            .liq_host
            .get()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".into())
    }

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

    /// 开播（station_activate on / boot 批量）：渲染 + 起管。
    pub async fn start(
        &self,
        st: &StdArc<ServerState>,
        row: &aiya_data::station::StationRow,
    ) -> Result<(), String> {
        // 幂等闸（审查 P1-1）：已在跑的频道重复 start（UI 双击/命令重放）
        // 直接返回——否则泄漏端口与回灌任务，还会短暂双编码器同写段目录
        if self
            .inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&row.id)
        {
            return Ok(());
        }
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

        // ---- 频道编排（审查 P1-5：节目表真正接进播放管线）----
        // 读歌单与节目表，生成每歌单 m3u 与 switch 块；无歌单退回全库随机 100
        let row_id = row.id.clone();
        let (playlists, schedules) = crate::state::db_lock(st)
            .send(move |conn| {
                let pls = aiya_data::station::list_playlists(conn, &row_id)?;
                let schs = aiya_data::station::list_schedules(conn, &row_id)?;
                Ok((pls, schs))
            })
            .map_err(|e| e.to_string())?;

        let mut program_sources = String::new();
        let mut pl_vars: HashMap<String, String> = HashMap::new();
        for (idx, pl) in playlists.iter().enumerate() {
            // send 闭包要 'static：先把 pl 的两处读出拷贝（kind/track_ids）
            let kind = pl.kind.clone();
            let pl_track_ids: Vec<String> = pl
                .track_ids_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let sid = row.id.clone();
            let entries: Vec<(String, String)> = crate::state::db_lock(st)
                .send(move |conn| {
                    match kind.as_str() {
                        // manual：按 track_ids 顺序（查全放 map，按列表序取）
                        "manual" => {
                            let mut by_id = HashMap::new();
                            if !pl_track_ids.is_empty() {
                                let placeholders = (1..=pl_track_ids.len())
                                    .map(|x| format!("?{x}"))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let sql = format!(
                                    "SELECT id, path, COALESCE(title, '') FROM tracks
                                     WHERE availability = 'online' AND id IN ({placeholders})"
                                );
                                let mut stmt = conn.prepare(&sql)?;
                                let rows = stmt.query_map(
                                    rusqlite::params_from_iter(pl_track_ids.iter()),
                                    |r| {
                                        Ok((
                                            r.get::<_, String>(0)?,
                                            r.get::<_, String>(1)?,
                                            r.get::<_, String>(2)?,
                                        ))
                                    },
                                )?;
                                for r in rows {
                                    let (id, path, title) = r?;
                                    by_id.insert(id, (path, title));
                                }
                            }
                            let ordered: Vec<(String, String)> = pl_track_ids
                                .iter()
                                .filter_map(|id| by_id.get(id).cloned())
                                .collect();
                            Ok(ordered)
                        }
                        // random_all：随机 100，排除 24h 内播过的（防重复窗口）
                        _ => random_entries(conn, &sid).map_err(aiya_data::DbError::Sql),
                    }
                })
                .map_err(|e| e.to_string())?;
            if entries.is_empty() {
                self.free_port(port);
                return Err(format!(
                    "歌单 {} 无可播曲目——频道 {} 未启动",
                    pl.name, row.name
                ));
            }
            let m3u_name = format!("pl{idx}.m3u");
            let m3u_path = hls_dir.join(&m3u_name);
            std::fs::write(&m3u_path, annotate_lines(&entries))
                .map_err(|e| format!("write m3u: {e}"))?;
            let var = format!("pl{idx}");
            program_sources += &format!(
                "pl{idx} = playlist(id=\"pl{idx}\", mode=\"randomize\", \"{liq_prefix}/{m3u_name}\")\n",
                liq_prefix = liq_prefix
            );
            pl_vars.insert(pl.id.clone(), var);
        }

        // 默认源（未排时段）：第一个 random_all 歌单 → 否则第一个歌单 →
        // 否则全库随机 100（无歌单的现状形态）
        let base_expr = playlists
            .iter()
            .find(|p| p.kind == "random_all")
            .or_else(|| playlists.first())
            .and_then(|p| pl_vars.get(&p.id).cloned());
        let (base_expr, program_sources) = match base_expr {
            Some(v) => (v, program_sources),
            None => {
                let sid = row.id.clone();
                let entries: Vec<(String, String)> = crate::state::db_lock(st)
                    .send(move |conn| random_entries(conn, &sid).map_err(aiya_data::DbError::Sql))
                    .map_err(|e| e.to_string())?;
                if entries.is_empty() {
                    self.free_port(port);
                    return Err(format!("曲库无在线曲目——频道 {} 未启动", row.name));
                }
                let m3u_path = hls_dir.join("default.m3u");
                std::fs::write(&m3u_path, annotate_lines(&entries))
                    .map_err(|e| format!("write m3u: {e}"))?;
                let base_m3u = format!("{liq_prefix}/default.m3u");
                (
                    "base".to_string(),
                    format!("base = playlist(id=\"base\", mode=\"randomize\", \"{base_m3u}\")\n"),
                )
            }
        };

        // switch 展开：每 (时段 × 星期) 一行，priority 序（DB 已 DESC）；
        // 跨零点（start>=end）拆"当日尾段 + 次日头段"两行；weekday 数字 =
        // bit0(周一)=1w … bit6(周日)=7w
        let mut rows: Vec<String> = Vec::new();
        for s in &schedules {
            let Some(var) = pl_vars.get(&s.playlist_id) else {
                continue;
            };
            let sh: u32 = s
                .start_hm
                .split(':')
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0);
            let eh: u32 = s
                .end_hm
                .split(':')
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0);
            for day in 0..7usize {
                if (s.days_mask & (1 << day)) == 0 {
                    continue;
                }
                let d = day + 1;
                if sh < eh {
                    rows.push(format!("  ({{{}h-{}h and {}w}}, {}),", sh, eh, d, var));
                } else {
                    rows.push(format!("  ({{{}h-24h and {}w}}, {}),", sh, d, var));
                    let nd = day % 7 + 1;
                    rows.push(format!("  ({{0h-{}h and {}w}}, {}),", eh, nd, var));
                }
            }
        }
        let schedule_block = if rows.is_empty() {
            String::new()
        } else {
            format!(
                "sch = switch(track_sensitive=false, [\n{}\n])",
                rows.join("\n")
            )
        };

        let text = render_liq(&LiqConfig {
            liq_dir: liq_prefix.clone(),
            telnet_port: port,
            telnet_bind,
            program_sources,
            base_expr,
            schedule_block,
        });

        let run = ChannelRun {
            dir: row.hls_dir.clone(),
            port,
            hls_dir: hls_dir.clone(),
            visibility: row.visibility.clone(),
            running: Arc::new(AtomicBool::new(false)),
            last_track: Arc::new(Mutex::new(None)),
            stop_tx: Mutex::new(None),
            poll_handle: Mutex::new(None),
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

        // 回灌轮询（两种形态同源：now.jsonl 由 liq 的 on_track 覆盖写）。
        // 句柄存进 ChannelRun：stop/覆盖前 abort（审查 P1-2——任务永生泄漏）
        let poll_last = Arc::clone(&run.last_track);
        let poll_bus = st.events.clone();
        let poll_dir = hls_dir.clone();
        let poll_sid = row.id.clone();
        let poll_handle = tokio::spawn(async move {
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
                    poll_bus.emit(
                        "stations",
                        &json!({ "stationId": poll_sid, "nowPlaying": t }),
                    );
                }
            }
        });
        *run.poll_handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(poll_handle);

        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(row.id.clone(), run);
        Ok(())
    }

    /// 停播（station_activate off）：InProcess 发 stop 信号；Sidecar 摘 ON 位
    /// （keeper 对账 kill）。段目录与配置保留——再开播热恢复。
    pub fn stop(&self, station_id: &str) -> Result<(), String> {
        let mode = *self.inner.mode.get().ok_or("station not booted")?;
        let (port, stop_tx, sidecar_dir, poll_handle) = {
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
            let sidecar_dir = c.hls_dir.clone();
            let poll_handle = c
                .poll_handle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            (port, stop_tx, sidecar_dir, poll_handle)
        };
        match mode {
            ControlMode::InProcess => {
                if let Some(tx) = stop_tx {
                    std::mem::drop(tx.send(()));
                }
            }
            ControlMode::Sidecar => {
                // join 只认相对名：hls_dir 是绝对路径
                let dir = sidecar_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let on = self
                    .inner
                    .hls_root
                    .get()
                    .map(|r| r.join("conf").join(&dir).join("ON"))
                    .unwrap_or_default();
                match std::fs::remove_file(&on) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("rm ON: {e}")),
                }
            }
        }
        if let Some(handle) = poll_handle {
            handle.abort();
        }
        self.free_port(port);
        self.inner
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(station_id);
        Ok(())
    }

    /// InProcess 守护：wait 循环 + 指数退避 + 连续崩溃上限；stop 信号到达即
    /// kill 子进程退出。
    fn spawn_guard(
        &self,
        liq_path: PathBuf,
        running: Arc<AtomicBool>,
        mut stop_rx: tokio::sync::mpsc::Receiver<()>,
    ) {
        tokio::spawn(async move {
            let mut backoff: u64 = 1;
            let mut crash_streak: u32 = 0;
            loop {
                // kill_on_drop（审查 P1-3）：server 关停/守护任务被 drop 时子
                // 进程随之终结，不留孤儿编码器占端口写目录
                let child = tokio::process::Command::new("liquidsoap")
                    .arg(&liq_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
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
                    r = child.wait() => r.map(|st| st.success()).map_err(|e| e.to_string()),
                    _ = stop_rx.recv() => Err("stop".into()),
                };
                let _ = child.kill().await;
                running.store(false, Ordering::SeqCst);
                match reason {
                    Err(e) if e == "stop" => return,
                    Err(e) => {
                        tracing::error!(error = %e, "station wait failed");
                        return;
                    }
                    Ok(false) => {
                        // 连续异常退出有上限：配置坏/端口被占这类不自愈错误
                        // 不做无限退避重试（审查 P1-3）
                        crash_streak += 1;
                        if crash_streak >= 10 {
                            tracing::error!(crash_streak, "station crash-loop, giving up");
                            return;
                        }
                    }
                    Ok(true) => crash_streak = 0,
                }
                tracing::warn!(
                    backoff_s = backoff,
                    crash_streak,
                    "station process exited, restarting"
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        });
    }

    /// `GET /hls/{dir}/ws`（§12.1）：升级后先发 hello（站单静态字段 + 当前
    /// 正在播），之后把 events 总线的 stations 通道原样转发（与 SSE 同一
    /// 状态源）。鉴权由 auth 中间件统一处理（public 豁免 / private ?token=）。
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
                            // SSE 载荷原样包进 track 帧（serde 重组——不再字符串
                            // 拼接嵌套，审查 P2-5 的双层 nowPlaying）
                            let mut v: Value = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            v["type"] = json!("track");
                            if sender
                                .send(Message::Text(v.to_string().into()))
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

/// 防重复窗口的随机选歌：24h 内播过的歌不进池（station_history）
fn random_entries(
    conn: &rusqlite::Connection,
    sid: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    // 防重复窗口：24h 内播过的歌不进随机池（station_history）
    let mut stmt = conn.prepare(
        "SELECT path, COALESCE(title, '') FROM tracks WHERE availability = 'online'
           AND id NOT IN (SELECT track_id FROM station_history
                          WHERE station_id = ?1 AND played_at >= ?2)
         ORDER BY RANDOM() LIMIT 100",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![sid, crate::state::now_secs() - 24 * 3600],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )?;
    let mut v = Vec::new();
    for r in rows {
        v.push(r?);
    }
    Ok(v)
}

/* ---- boot ---- */

/// 启动钩子：定形态/段根，把 DB 里 active=1 的频道逐个起起来。
/// 失败只记日志不拦启动——推流是附属面，不能让频道问题挡住曲库服务。
pub async fn boot(st: &StdArc<ServerState>, data_root: &Path) -> Result<(), String> {
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

    let all: Vec<aiya_data::station::StationRow> = crate::state::db_lock(st)
        .send(|conn| aiya_data::station::list_stations(conn))
        .map_err(|e| e.to_string())?;

    // GC：段目录无对应 station 行 = 删除残留（频道删除/异常退出）——
    // boot 时机单点执行。精确形态 ch+8hex（审查 P2-2：防误伤 AIYA_HLS_ROOT
    // 下 ch 开头的无关目录）
    let known: std::collections::HashSet<String> = all.iter().map(|r| r.hls_dir.clone()).collect();
    if let Ok(entries) = std::fs::read_dir(&hls_root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_ours = name.len() == 10
                && name.starts_with("ch")
                && name[2..].bytes().all(|b| b.is_ascii_hexdigit());
            if !known.contains(&name) && is_ours {
                let p = e.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                    tracing::info!(dir = %name, "gc: removed orphan station dir");
                }
            }
        }
    }

    let active: Vec<_> = all.into_iter().filter(|r| r.active).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LiqConfig {
        LiqConfig {
            liq_dir: "/hls/ch1".into(),
            telnet_port: 12701,
            telnet_bind: "0.0.0.0",
            program_sources:
                "pl0 = playlist(id=\"pl0\", mode=\"randomize\", \"/hls/ch1/pl0.m3u\")\n".into(),
            base_expr: "pl0".into(),
            schedule_block: "sch = switch(track_sensitive=false, [\n  ({8h-10h and 1w}, pl0),\n])"
                .into(),
        }
    }

    #[test]
    fn liq_template_2_4_6_key_lines() {
        let t = render_liq(&cfg());
        // 2.4.6 实测形态（§13.2/§14 逐条实证）
        assert!(
            t.contains("crossfade(duration=2., sch)"),
            "节目表链过 crossfade：{t}"
        );
        assert!(
            t.contains("\"/hls/ch1\""),
            "目录位置参（无 directory 标签）"
        );
        assert!(
            t.contains(
                "
src.on_track(synchronous=true, now_write)"
            ),
            "副作用调用（行首，不赋值——unit 赋 src 会炸 fallback）"
        );
        assert!(!t.contains("src = src.on_track"), "禁止赋值形态");
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
        assert!(t.contains("pl0 = playlist"), "歌单源定义");
        assert!(
            t.contains("sch = switch(track_sensitive=false,"),
            "switch 展开"
        );
        assert!(t.contains("({8h-10h and 1w}, pl0)"), "时段×星期谓词行");
        // 无节目表形态：src 直接走 base
        let mut c2 = cfg();
        c2.schedule_block = String::new();
        c2.base_expr = "base".into();
        c2.program_sources =
            "base = playlist(id=\"base\", mode=\"randomize\", \"/hls/ch1/default.m3u\")\n".into();
        let t2 = render_liq(&c2);
        assert!(
            t2.contains("crossfade(duration=2., base)"),
            "无节目表走 base 直连"
        );
        assert!(!t2.contains("sch ="), "无 switch 块");
    }
}
