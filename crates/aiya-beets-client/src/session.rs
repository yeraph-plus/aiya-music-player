//! 进程生命周期与 NDJSON 行协议。
//!
//! 这一层是 client 的心脏，也是它与"环境知识"完全无关的部分：起进程、写一行
//! 请求、逐行读事件、超时收尸、会话互斥、取消。它不知道对面是谁，也不知道
//! 请求里装的是什么业务——只认「一行一个 JSON」这一条铁律。
//!
//! 三条实测得来的硬约束（别改回去）：
//!
//! 1. **按字节读再显式 `from_utf8`**。`BufRead::lines()` 遇到非法字节直接
//!    返回 Err 并结束迭代，调用方只看到"没有输出了"——真正的编码问题被埋掉。
//!    这里逐行判编码，非法行记日志后跳过，会话继续。
//! 2. **超时必须收尸**。只 return 不 kill，卡死的子进程会一直占着管道；
//!    调用方等到的是"没响应"，留下的是僵尸。
//! 3. **spawn 之前占会话位**。此前是 spawn 之后才 acquire，并发的第二个请求
//!    会照样起进程、请求行照样进对方队列照常执行（写标签、动数据），却没有
//!    reader 转发事件——既脏又泄漏。

use std::io::{BufRead, Read, Write};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::ToolError;
use crate::launch::Launcher;
use crate::proto::{Envelope, StatusReport};

/// 会话空闲看门狗：连续这么久没有任何一行输出就判卡死。
///
/// 正常路径每条记录都会带回执（解析一条报一条），静默十分钟只可能是死锁。
/// 没有这道闸时，一次卡死会让会话位永久占死，之后所有操作都报"已有任务"。
pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// reader 的等待粒度。切成 1s 是为了能周期性检查"取消强杀"的截止时刻
/// （见 [`cancel`]）；代价只是空闲期每秒醒一次，可忽略。
const RECV_WAIT_CHUNK: Duration = Duration::from_secs(1);

/// 事件出口。宿主用自己的方式实现（Tauri 事件、日志、测试收集器……）。
///
/// `channel` 用 `&'static str`：通道名是宿主的常量子集，运行期不该拼字符串。
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, channel: &'static str, payload: Value);
}

/// 吞掉所有事件的实现，供测试与"不需要进度"的调用方使用。
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _channel: &'static str, _payload: Value) {}
}

/// 活跃会话的写入口（取消通道 + 保活句柄）。
pub type SessionStdin = ChildStdin;

fn slot() -> &'static Mutex<Option<(u64, SessionStdin)>> {
    static SLOT: OnceLock<Mutex<Option<(u64, SessionStdin)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

/// 进程级世代计数器：每次 acquire 递增。会话槽里连同世代号一起存，
/// 旧会话 reader 的迟到 release 靠世代不匹配变成无害 no-op——
/// 否则它会偷走新会话的 stdin 并清掉 busy 位（两个 beets 进程并发写同一 sqlite）。
fn gen_counter() -> &'static AtomicU64 {
    static GEN: OnceLock<AtomicU64> = OnceLock::new();
    GEN.get_or_init(|| AtomicU64::new(0))
}

/// 抢占会话位（spawn 之前调用）。已有会话时返回 false。
///
/// 用 compare_exchange 而不是 load+store：两个线程同时进来只有一个能拿到，
/// 这是并发保护的全部意义。
pub fn reserve() -> bool {
    flag()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// 当前是否有会话占着位子（只读）。
///
/// 宿主用它做"让位"判断：自动刷新每 30 s 就会问一次，撞上导入/整理会话时
/// 必须安静跳过——否则两个 sidecar 会同时读同一个 beets 库（sqlite 锁竞争），
/// 而且自动刷新发出去的 `librarySynced` 会让正在应用的那条链路误以为
/// "对齐完成了"，把界面的 applying 状态提前收掉。
pub fn busy() -> bool {
    flag().load(Ordering::SeqCst)
}

/// 把写入口放进槽位。flag 已由 `reserve` 占住，正常不会失败。
///
/// 返回本次会话的**世代号**：此后该会话的所有清理（reader 收尾、失败路径）
/// 都必须带这个号，[`release`] 只在世代匹配时生效。
pub fn acquire(stdin: SessionStdin) -> Result<u64, ToolError> {
    let gen = gen_counter().fetch_add(1, Ordering::SeqCst);
    let mut guard = slot().lock().map_err(|_| ToolError::Io("会话槽已中毒".into()))?;
    if guard.is_some() {
        return Err(ToolError::Busy);
    }
    *guard = Some((gen, stdin));
    Ok(gen)
}

/// 释放会话位并取回写入口（reader / 失败路径调用）。**必须带自己的世代号**。
///
/// 只在槽内世代与 `gen` 匹配时清位并取走 stdin；不匹配（本会话已被取消、
/// 槽里已是新会话）或槽空时是 no-op——旧 reader 的迟到清理因此无害。
pub fn release(gen: u64) -> Option<SessionStdin> {
    let mut guard = slot().lock().ok()?;
    match guard.as_ref() {
        // 世代匹配 → 这确实还是本会话：清位 + 取 stdin（同一把锁内完成，
        // 中间不会插进新会话的 acquire）
        Some((slot_gen, _)) if *slot_gen == gen => {
            flag().store(false, Ordering::SeqCst);
            guard.take().map(|(_, stdin)| stdin)
        }
        // 世代不匹配/槽空：本会话已被 cancel 或替换，什么都不动
        _ => None,
    }
}

/// 无世代号的释放：一次性查询、以及 reserve 之后 acquire 之前失败的路径。
///
/// 这类调用者从没进过槽位，只需在"槽确实还空着"时把 busy 位还回去；
/// 槽被真会话占着时绝不动它（那个位子属于槽里的会话）。
pub(crate) fn release_unacquired() {
    if let Ok(guard) = slot().lock() {
        if guard.is_none() {
            flag().store(false, Ordering::SeqCst);
        }
    }
}

/// 取消兜底：cancel 之后等这么久还没收尾（工具无视 EOF），reader 强杀进程。
/// 纯协作式取消（只写一行 + 关 stdin）对不守规矩的工具没有约束力。
pub const CANCEL_KILL_TIMEOUT: Duration = Duration::from_secs(15);

/// 取消的截止时刻，按世代号记账：reader 只对自己的世代生效。
/// 不带世代的话，旧会话的取消截止会误杀后继的新会话。
fn cancel_deadline() -> &'static Mutex<Option<(u64, Instant)>> {
    static DL: OnceLock<Mutex<Option<(u64, Instant)>>> = OnceLock::new();
    DL.get_or_init(|| Mutex::new(None))
}

fn cancel_deadline_for(gen: u64) -> Option<Instant> {
    match cancel_deadline().lock() {
        Ok(g) => match *g {
            Some((slot_gen, t)) if slot_gen == gen => Some(t),
            _ => None,
        },
        Err(_) => None, // 中毒时放弃兜底强杀，退化为纯协作取消
    }
}

/// 取消当前会话：写一行 cancel（对面在下个检查点收尾并发终态事件），
/// 再关 stdin 让它在收尾后 EOF 退出，同时清掉 busy 位。
///
/// 取的是**槽内当前会话**的 stdin（无论世代——取消永远针对"现在正跑的这个"）。
/// 槽空返回 false（幂等）。
pub fn cancel() -> bool {
    let mut stdin = {
        let mut guard = match slot().lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match guard.take() {
            Some((gen, s)) => {
                flag().store(false, Ordering::SeqCst);
                // 记录强杀截止：工具若无视 EOF，reader 到点 kill+wait
                if let Ok(mut dl) = cancel_deadline().lock() {
                    *dl = Some((gen, Instant::now() + CANCEL_KILL_TIMEOUT));
                }
                s
            }
            None => return false,
        }
    };
    {
        let _ = writeln!(stdin, r#"{{"id":0,"cmd":"cancel"}}"#);
    }
    drop(stdin);
    true
}

/// 起一个短进程并写入一条请求；stdin 返回给调用方保活（取消要用）。
///
/// stdout 交给调用方接管：一次性查询自己读完就走，会话式由 `spawn_reader`
/// 起转发线程。
pub fn spawn_session(
    launcher: &Launcher,
    request: &Value,
) -> Result<(Child, SessionStdin), ToolError> {
    // 握手声明：告诉对面"我说的是 vN"。放在 client 里而不是让宿主每个请求
    // 自己拼——版本是协议层的事实，宿主不该重复知道。
    // 调用方显式给了就尊重它（测试要造版本不符的会话）。
    let mut request = request.clone();
    if let Some(obj) = request.as_object_mut() {
        obj.entry("clientProtocol")
            .or_insert_with(|| Value::from(crate::proto::PROTOCOL_VERSION));
    }
    let request = &request;
    let mut child = launcher
        .command()
        .spawn()
        .map_err(|e| ToolError::Spawn { program: launcher.program.clone(), message: e.to_string() })?;

    // 写失败必须收尸：只 return 的话子进程挂着管道变成孤儿（超时收尸是
    // 读侧的规矩，写侧同样适用）。
    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Io("stdin 未建立".into()));
        }
    };
    if let Err(e) = writeln!(stdin, "{request}") {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ToolError::Io(format!("写请求失败: {e}")));
    }
    Ok((child, stdin))
}

/// 单行输出上限。对面正常只说 NDJSON；一条不换行的失控输出没有上限会把
/// 内存吃穿，超限即判"已不是在说协议"，终止读取（会话随后按异常收尾）。
const MAX_LINE_BYTES: usize = 10 * 1024 * 1024;

/// 逐行读一个管道，把 UTF-8 合法行送进 channel；非法行记日志后跳过。
///
/// 单独抽出来的原因：一次性查询、会话转发、批量收集三处读法完全一样，
/// 而编码处理是最容易各自漂移的那种细节（此前就漏过一次）。
pub fn spawn_line_reader<R: Read + Send + 'static>(
    reader: R,
    tx: std::sync::mpsc::Sender<String>,
    thread_name: &'static str,
) -> Result<(), ToolError> {
    std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let mut reader = std::io::BufReader::new(reader);
            let mut buf: Vec<u8> = Vec::new();
            loop {
                buf.clear();
                // take 上限 = MAX+1：单行内容恰好到上限时仍能读到换行，
                // 超限时缓冲最多 10MB+1，内存有界（read_until 本身没有上限参数）
                let mut take = reader.by_ref().take((MAX_LINE_BYTES + 1) as u64);
                match take.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "读取外部工具输出失败");
                        break;
                    }
                }
                let content_len = if buf.ends_with(b"\n") { buf.len() - 1 } else { buf.len() };
                if content_len > MAX_LINE_BYTES {
                    tracing::error!(
                        len = content_len,
                        cap = MAX_LINE_BYTES,
                        "外部工具单行输出超过上限，终止读取"
                    );
                    break;
                }
                match std::str::from_utf8(&buf) {
                    Ok(s) => {
                        let s = s.trim_end();
                        if tx.send(s.to_string()).is_err() {
                            break; // 接收端走了，别再读
                        }
                    }
                    Err(e) => tracing::warn!(
                        valid_up_to = e.valid_up_to(),
                        len = buf.len(),
                        "外部工具输出含非法 UTF-8 字节，丢弃该行"
                    ),
                }
            }
        })
        .map(|_| ())
        .map_err(|e| ToolError::Io(format!("起读线程失败: {e}")))
}

/// 从行通道里等**第一条可解析的 JSON 行**。`timeout` 是整体截止（不是单行）：
/// 工具初始化时会先打人类可读的提示（实测：新库首写会打迁移日志），那一行
/// 不是协议内容，跳过后继续等——对齐 `spawn_reader` 逐行试解析的容错行为。
fn recv_first_json(rx: &std::sync::mpsc::Receiver<String>, timeout: Duration) -> Result<Value, ToolError> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(ToolError::Timeout { timeout });
        }
        let line = match rx.recv_timeout(deadline - now) {
            Ok(l) => l,
            Err(RecvTimeoutError::Timeout) => return Err(ToolError::Timeout { timeout }),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ToolError::Protocol("进程在应答前退出".into()));
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => return Ok(v),
            Err(e) => tracing::warn!(%e, %trimmed, "一次性查询收到非 JSON 行，跳过"),
        }
    }
}

/// 发一条一次性请求并读第一条可解析的应答行（内部 API 的同步形态）。
fn request_once(launcher: &Launcher, request: &Value, timeout: Duration) -> Result<Value, ToolError> {
    let (mut child, stdin) = spawn_session(launcher, request)?;
    // 一次性查询：写完整即关，让对面读到 EOF 后自己收尾
    drop(stdin);

    let result = (|| -> Result<Value, ToolError> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::Io("stdout 未建立".into()))?;
        let (tx, rx) = channel::<String>();
        spawn_line_reader(stdout, tx, "tool-once-recv")?;
        recv_first_json(&rx, timeout)
    })();

    // 无论成败都收尸：超时/提前退出/正常拿到应答，都不留僵尸
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// 发一次 `status` 并解析成报告。
///
/// 应答 `ok: false` 时返回 `Rejected`——把工具自己的 code/stage/消息原样带出去，
/// 宿主不翻译（工具自己的诊断比宿主的猜测准）。
///
/// 与会话式操作共用同一把进程级互斥：一次性查询同样在起 beets 进程、读同一
/// 个 sqlite 库，绕过会话槽会让 `busy()`（刷新让位判断）对它视而不见。
pub fn query_status(launcher: &Launcher, timeout: Duration) -> Result<StatusReport, ToolError> {
    if !reserve() {
        return Err(ToolError::Busy);
    }
    let raw = request_once(
        launcher,
        &serde_json::json!({
            "id": 1,
            "cmd": "status",
            "clientProtocol": crate::proto::PROTOCOL_VERSION,
        }),
        timeout,
    );
    let result = raw.and_then(|raw| {
        let env: Envelope = serde_json::from_value(raw)
            .map_err(|e| ToolError::Protocol(format!("应答信封形状不对: {e}")))?;
        if !env.ok {
            let err = env.error.unwrap_or_else(|| crate::proto::EnvelopeError {
                code: "E_UNKNOWN".into(),
                stage: "unknown".into(),
                beets_message: "no error detail".into(),
            });
            return Err(ToolError::Rejected {
                code: err.code,
                stage: err.stage,
                message: err.beets_message,
            });
        }
        match env.data {
            Some(data) => serde_json::from_value(data)
                .map_err(|e| ToolError::Protocol(format!("status 字段形状不对: {e}"))),
            None => Ok(StatusReport::default()),
        }
    });
    release_unacquired();
    result
}

/// 发一次 `plugin_catalog`，返回市场目录数组（每个元素是 market_report 的一行）。
///
/// 与 `query_status` 同款一次性短进程查询：目录是静态白名单 + 三态检测，
/// 低频读，不值得为它养常驻会话。元素保持 `Value` 不定形——字段归 DTO 解释
/// （`BeetsMarketItem`），client crate 不掺 UI 语义。
/// 同样走会话互斥（理由见 `query_status`）。
pub fn query_plugin_catalog(
    launcher: &Launcher,
    timeout: Duration,
) -> Result<Vec<Value>, ToolError> {
    if !reserve() {
        return Err(ToolError::Busy);
    }
    let raw = request_once(
        launcher,
        &serde_json::json!({
            "id": 1,
            "cmd": "plugin_catalog",
            "clientProtocol": crate::proto::PROTOCOL_VERSION,
        }),
        timeout,
    );
    let result = raw.and_then(|raw| {
        let env: Envelope = serde_json::from_value(raw)
            .map_err(|e| ToolError::Protocol(format!("应答信封形状不对: {e}")))?;
        if !env.ok {
            let err = env.error.unwrap_or_else(|| crate::proto::EnvelopeError {
                code: "E_UNKNOWN".into(),
                stage: "unknown".into(),
                beets_message: "no error detail".into(),
            });
            return Err(ToolError::Rejected {
                code: err.code,
                stage: err.stage,
                message: err.beets_message,
            });
        }
        let data = env.data.unwrap_or(serde_json::Value::Null);
        let catalog = data
            .get("catalog")
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        serde_json::from_value(catalog)
            .map_err(|e| ToolError::Protocol(format!("catalog 字段形状不对: {e}")))
    });
    release_unacquired();
    result
}

/// 会话式召回的终态事件名。见到任意一个就收尾——**不能靠进程退出判断**
/// （正常收尾和崩溃退出长得一样，混为一谈会漏掉"半份结果"）。
/// 会话终态事件名。读到其中之一就收尾（关进程、放槽位、调 `on_exit`）。
///
/// `pub` 是为了让 `tests/proto_mirror.rs` 拿它与镜像 schema 里的 `type`
/// 常量集合比对——改名而忘了改这里，症状是"会话永远不结束"，很难现场定位。
pub const TERMINAL_EVENTS: &[&str] =
    &["done", "applyDone", "enrichDone", "cancelled", "sessionFailed", "pluginDone"];

/// 起会话转发线程：把每一行 JSON 转发给 `sink`，见终态事件后收尾。
///
/// `gen` 是本会话的世代号（[`acquire`] 的返回值）：收尾时 `release(gen)` 只在
/// 槽内世代仍是自己时生效——旧 reader 的迟到清理不会偷走后继会话的槽位。
///
/// `on_exit` 一定在"会话真正结束之后"调用（收尸完、槽位已释放），用于让
/// 调用方在数据落盘后做后续动作（比如对账）。它跑在自己的线程里，不阻塞读循环。
pub fn spawn_reader(
    mut child: Child,
    gen: u64,
    sink: Arc<dyn EventSink>,
    channel_name: &'static str,
    on_exit: Box<dyn FnOnce() + Send + 'static>,
) -> Result<(), ToolError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Io("stdout 未建立".into()))?;

    let (line_tx, line_rx) = channel::<String>();
    if let Err(e) = spawn_line_reader(stdout, line_tx, "tool-pipe-recv") {
        let _ = child.kill();
        let _ = child.wait();
        let _ = release(gen);
        return Err(e);
    }

    // Arc<Mutex<Child>>：会话线程持有一份（kill/wait），失败路径也持有一份收尸
    let child = Arc::new(Mutex::new(child));
    let child_for_fail = Arc::clone(&child);

    std::thread::Builder::new()
        .name("tool-session-reader".into())
        .spawn(move || {
            // 空闲预算：连续这么久没有任何一行输出就判卡死（看门狗）。
            let mut idle_left = SESSION_IDLE_TIMEOUT;
            // 是否见过工具的终态事件：区分"正常收尾"与"崩溃退出"——
            // 正常收尾和崩溃退出在管道上长得一样（都是 EOF）。
            let mut saw_terminal = false;
            let mut crashed = false;
            loop {
                // 等待粒度切成 ≤1s：取消兜底（到点强杀）需要周期性醒来看看截止时刻
                let chunk = RECV_WAIT_CHUNK.min(idle_left);
                match line_rx.recv_timeout(chunk) {
                    Ok(line) => {
                        idle_left = SESSION_IDLE_TIMEOUT; // 有输出就续命
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(trimmed) {
                            Ok(evt) => {
                                let terminal = evt
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .is_some_and(|t| TERMINAL_EVENTS.contains(&t));
                                if terminal {
                                    saw_terminal = true;
                                }
                                sink.emit(channel_name, evt);
                                if terminal {
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(%e, %trimmed, "非法事件行，忽略");
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // 取消兜底：工具无视 EOF 超过 CANCEL_KILL_TIMEOUT → 强杀
                        if let Some(deadline) = cancel_deadline_for(gen) {
                            if Instant::now() >= deadline {
                                tracing::error!(
                                    timeout_secs = CANCEL_KILL_TIMEOUT.as_secs(),
                                    "取消后工具未按时收尾，强制结束会话进程"
                                );
                                if let Ok(mut c) = child.lock() {
                                    let _ = c.kill();
                                    let _ = c.wait();
                                }
                                sink.emit(
                                    channel_name,
                                    serde_json::json!({
                                        "type": "sessionFailed",
                                        "error": {
                                            "code": "E_SESSION_CANCEL_TIMEOUT",
                                            "stage": "cancel",
                                            "beets_message":
                                                "取消后工具未在时限内收尾，进程已被强制结束",
                                        },
                                    }),
                                );
                                break;
                            }
                            continue; // 还没到截止，继续等（不算空闲超时）
                        }
                        idle_left -= chunk;
                        if idle_left.is_zero() {
                            tracing::error!("会话空闲超时（10 分钟无输出），看门狗终止");
                            if let Ok(mut c) = child.lock() {
                                let _ = c.kill();
                            }
                            sink.emit(
                                channel_name,
                                serde_json::json!({
                                    "type": "sessionFailed",
                                    "error": {
                                        "code": "E_SESSION_TIMEOUT",
                                        "stage": "watchdog",
                                        "beets_message": "会话 10 分钟无输出，已被看门狗终止",
                                    },
                                }),
                            );
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        // 进程退出。没见过终态事件 = 崩溃（或被外部杀掉），
                        // 补一条失败事件让宿主能区分"半份结果"与"正常收尾"。
                        crashed = !saw_terminal;
                        break;
                    }
                }
            }
            // 关 stdin → 对面读到 EOF 后退出；wait 收尸防僵尸。
            // 世代号兜底：本会话已被 cancel（槽已空）时这里是 no-op。
            drop(release(gen));
            if let Ok(mut c) = child.lock() {
                let _ = c.wait();
            }
            if crashed {
                sink.emit(
                    channel_name,
                    serde_json::json!({
                        "type": "sessionFailed",
                        "error": {
                            "code": "E_SESSION_CRASHED",
                            "stage": "reader",
                            "beets_message": "工具进程在发出终态事件前退出",
                        },
                    }),
                );
            }
            on_exit();
            sink.emit(channel_name, serde_json::json!({ "type": "sessionEnd" }));
        })
        .map(|_| ())
        .map_err(|e| {
            if let Ok(mut c) = child_for_fail.lock() {
                let _ = c.kill();
                let _ = c.wait();
            }
            drop(release(gen));
            ToolError::Io(format!("起会话线程失败: {e}"))
        })
}

/// 会话槽是**进程级静态状态**，测试并行跑会互相踩（A 占着位子时 B 期望
/// 拿到 Spawn 却拿到 Busy）。所有碰会话槽的测试都必须先拿这把锁。
///
/// 中毒也要能继续：某个测试 panic 之后，后面的测试还得跑（`into_inner`
/// 而不是 unwrap），否则一次失败会连坐整个模块。
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex as StdMutex;

    /// 收集事件的测试出口。
    #[derive(Default)]
    struct CollectSink(StdMutex<Vec<(String, Value)>>);

    impl EventSink for CollectSink {
        fn emit(&self, channel: &'static str, payload: Value) {
            self.0.lock().unwrap().push((channel.to_string(), payload));
        }
    }

    /// 起一个"从 stdin 读到 EOF 就退出"的子进程，给会话槽测试提供真实的
    /// `ChildStdin`（槽位存的是进程管道，没法凭空造）。Windows 用 `sort`、
    /// Unix 用 `cat`：行为等价，EOF 后自行退出。
    fn spawn_stdin_holder() -> Child {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "sort"]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("cat");
            c
        };
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("起测试用 stdin 持有进程失败")
    }

    /// 清掉上个测试可能残留的会话状态（静态槽是进程级的，测试串行持锁跑）。
    fn reset_slot() {
        let _ = cancel();
        release_unacquired();
    }

    /// 收尸测试子进程，防僵尸。
    fn reap(children: &mut [Child]) {
        for c in children.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    #[test]
    fn reserve_acquire_release_round_trip() {
        let _g = test_lock();
        reset_slot();
        assert!(!busy());
        assert!(reserve(), "空槽必须能占");
        assert!(!reserve(), "已占用时第二次必须失败");
        let mut kids = vec![spawn_stdin_holder()];
        let gen = acquire(kids[0].stdin.take().unwrap()).unwrap();
        let back = release(gen).expect("当前世代的 release 必须取回 stdin");
        drop(back);
        assert!(!busy(), "release 必须清掉 busy 位");
        assert!(reserve(), "释放后必须能再占");
        release_unacquired();
        assert!(!busy());
        reap(&mut kids);
    }

    /// P0 回归：取消长会话后新会话已入槽，旧 reader 的迟到 release（旧世代）
    /// 不得偷走新会话的 stdin、不得清掉 busy 位、新会话必须仍可取消。
    /// （旧实现 release() 无条件清位 + take 槽位，正是这里模拟的时序会翻车。）
    #[test]
    fn stale_release_cannot_steal_new_generation() {
        let _g = test_lock();
        reset_slot();

        // 旧会话 A：reserve + acquire
        assert!(reserve());
        let mut kids = vec![spawn_stdin_holder()];
        let gen_a = acquire(kids[0].stdin.take().unwrap()).unwrap();

        // 用户取消 A（cancel 清槽、关 stdin、清 busy 位）
        assert!(cancel());

        // 新会话 B：reserve + acquire（旧 reader 此刻还没收尾）
        assert!(reserve(), "cancel 后位子必须能立刻再占");
        kids.push(spawn_stdin_holder());
        let gen_b = acquire(kids[1].stdin.take().unwrap()).unwrap();

        // 旧 reader 迟到收尾：release(旧世代) 必须是 no-op
        assert!(release(gen_a).is_none(), "旧世代的 release 不得偷走新会话的 stdin");
        assert!(busy(), "新会话的 busy 位必须还在");
        assert!(cancel(), "新会话必须仍可取消");

        // B 的 reader 正常收尾：cancel 已清槽，release(gen_b) 同样是 no-op
        assert!(release(gen_b).is_none());
        assert!(!busy());
        reap(&mut kids);
    }

    /// release_unacquired（一次性查询路径）在槽被真会话占着时不得清它的位。
    #[test]
    fn release_unacquired_respects_occupied_slot() {
        let _g = test_lock();
        reset_slot();
        assert!(reserve());
        let mut kids = vec![spawn_stdin_holder()];
        let gen = acquire(kids[0].stdin.take().unwrap()).unwrap();
        release_unacquired();
        assert!(busy(), "槽被占时 release_unacquired 不得清 busy 位");
        assert!(release(gen).is_some(), "真会话的释放仍应取回 stdin");
        assert!(!busy());
        reap(&mut kids);
    }

    /// cancel 给"被取消的那个世代"记强杀截止时刻；别的世代查不到它。
    #[test]
    fn cancel_arms_deadline_for_cancelled_generation_only() {
        let _g = test_lock();
        reset_slot();
        assert!(reserve());
        let mut kids = vec![spawn_stdin_holder()];
        let gen = acquire(kids[0].stdin.take().unwrap()).unwrap();
        assert!(cancel_deadline_for(gen).is_none(), "取消前不应有截止时刻");
        assert!(cancel());
        let dl = cancel_deadline_for(gen).expect("cancel 必须记下截止时刻");
        assert!(dl > Instant::now(), "截止时刻必须在未来");
        assert!(
            dl <= Instant::now() + CANCEL_KILL_TIMEOUT,
            "截止时刻不应超过取消兜底时限"
        );
        // 别的世代（新会话）不得继承这个截止
        assert!(cancel_deadline_for(gen + 1).is_none());
        reap(&mut kids);
    }

    #[test]
    fn cancel_without_session_is_false() {
        let _g = test_lock();
        reset_slot();
        assert!(!cancel(), "没有会话时取消是幂等的 false");
    }

    /// 真起一个子进程验协议：用 `echo` 之类保证跨平台可用的命令不存在，
    /// 所以这里不 spawn，只验纯逻辑（真进程验证在 src-tauri 的 dev 测试里）。
    #[test]
    fn terminal_event_set_is_the_documented_five() {
        assert_eq!(
            TERMINAL_EVENTS,
            &["done", "applyDone", "enrichDone", "cancelled", "sessionFailed", "pluginDone"]
        );
        // sessionEnd 是收尾后宿主自己补的，绝不能出现在终态集合里
        assert!(!TERMINAL_EVENTS.contains(&"sessionEnd"));
    }

    /// 非法行不该让会话死掉：读线程只丢行，不中断。
    #[test]
    fn line_reader_skips_invalid_utf8_and_keeps_going() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"{\"type\":\"item\"}\n");
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']); // 非法 UTF-8 行
        bytes.extend_from_slice(b"{\"type\":\"done\"}\n");
        let (tx, rx) = channel::<String>();
        spawn_line_reader(std::io::Cursor::new(bytes), tx, "test-recv").unwrap();
        let got: Vec<String> = rx.iter().collect();
        assert_eq!(got, vec![r#"{"type":"item"}"#, r#"{"type":"done"}"#]);
    }

    #[test]
    fn line_reader_stops_when_receiver_drops() {
        let payload = b"{\"a\":1}\n".repeat(1000);
        let (tx, rx) = channel::<String>();
        spawn_line_reader(std::io::Cursor::new(payload), tx, "test-recv2").unwrap();
        drop(rx); // 接收端消失后读线程应自行结束，不 panic
    }

    /// 恰好在上限内的单行必须原样通过（上限按"不含换行的内容长度"计）。
    #[test]
    fn line_reader_allows_line_at_cap() {
        let mut bytes = vec![b'a'; MAX_LINE_BYTES];
        bytes.push(b'\n');
        let (tx, rx) = channel::<String>();
        spawn_line_reader(std::io::Cursor::new(bytes), tx, "test-cap-ok").unwrap();
        let got: Vec<String> = rx.iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), MAX_LINE_BYTES);
    }

    /// 超限行终止整个读循环（不是丢行了事）：失控输出不该被慢慢吞。
    #[test]
    fn line_reader_breaks_on_oversized_line() {
        let mut bytes = vec![b'a'; MAX_LINE_BYTES + 2];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"{\"ok\":true}\n");
        let (tx, rx) = channel::<String>();
        spawn_line_reader(std::io::Cursor::new(bytes), tx, "test-cap").unwrap();
        // 超限后 tx 被丢弃：iter 立刻结束，且后续合法行不再读
        assert_eq!(rx.iter().count(), 0, "超限行必须终止读取");
    }

    /// 一次性应答：人类可读首行（工具初始化提示）必须被跳过，继续等真应答。
    #[test]
    fn recv_first_json_skips_human_readable_lines() {
        let (tx, rx) = channel::<String>();
        tx.send("正在迁移数据库 v23 -> v24 ...".into()).unwrap();
        tx.send(r#"{"ok":true,"data":{"x":1}}"#.into()).unwrap();
        let v = recv_first_json(&rx, Duration::from_secs(1)).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn recv_first_json_times_out_without_json() {
        let (tx, rx) = channel::<String>();
        tx.send("只有人类可读输出".into()).unwrap();
        let err = recv_first_json(&rx, Duration::from_millis(50)).unwrap_err();
        assert!(matches!(err, ToolError::Timeout { .. }), "实际 {err:?}");
    }

    #[test]
    fn recv_first_json_reports_protocol_on_disconnected() {
        let (tx, rx) = channel::<String>();
        tx.send("noise".into()).unwrap();
        drop(tx); // 进程带着没说的话退出了
        let err = recv_first_json(&rx, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, ToolError::Protocol(_)), "实际 {err:?}");
    }

    #[test]
    fn collect_sink_records_channel() {
        let sink = CollectSink::default();
        sink.emit("ch", serde_json::json!({"type": "x"}));
        let got = sink.0.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "ch");
    }
}
