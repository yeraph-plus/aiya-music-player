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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

use crate::error::ToolError;
use crate::launch::Launcher;
use crate::proto::{Envelope, StatusReport};

/// 会话空闲看门狗：连续这么久没有任何一行输出就判卡死。
///
/// 正常路径每条记录都会带回执（解析一条报一条），静默十分钟只可能是死锁。
/// 没有这道闸时，一次卡死会让会话位永久占死，之后所有操作都报"已有任务"。
pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

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

fn slot() -> &'static Mutex<Option<SessionStdin>> {
    static SLOT: OnceLock<Mutex<Option<SessionStdin>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
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
pub fn acquire(stdin: SessionStdin) -> Result<(), ToolError> {
    let mut guard = slot().lock().map_err(|_| ToolError::Io("会话槽已中毒".into()))?;
    if guard.is_some() {
        return Err(ToolError::Busy);
    }
    *guard = Some(stdin);
    Ok(())
}

/// 释放会话位并取回写入口（reader / cancel / 失败路径调用）。
pub fn release() -> Option<SessionStdin> {
    flag().store(false, Ordering::SeqCst);
    slot().lock().ok().and_then(|mut g| g.take())
}

/// 取消当前会话：先写一行 cancel（对面在下个检查点收尾并发终态事件），
/// 再关 stdin 让它在收尾后 EOF 退出。没有活跃会话时返回 false（幂等）。
pub fn cancel() -> bool {
    let Some(mut stdin) = release() else {
        return false;
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

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ToolError::Io("stdin 未建立".into()))?;
    writeln!(stdin, "{request}")
        .map_err(|e| ToolError::Io(format!("写请求失败: {e}")))?;
    Ok((child, stdin))
}

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
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "读取外部工具输出失败");
                        break;
                    }
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

/// 发一条一次性请求并读第一条可解析的应答行（内部 API 的同步形态）。
///
/// 容错非 JSON 行是必需的：工具首次初始化时会往 stdout 打人类可读的提示
/// （实测：新库首写会打迁移日志），那一行不是协议内容，跳过即可。
fn request_once(launcher: &Launcher, request: &Value, timeout: Duration) -> Result<Value, ToolError> {
    let (mut child, stdin) = spawn_session(launcher, request)?;
    // 一次性查询：写完整即关，让对面读到 EOF 后自己收尾
    drop(stdin);

    let (tx, rx) = channel::<String>();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Io("stdout 未建立".into()))?;
    spawn_line_reader(stdout, tx, "tool-once-recv")?;

    let line = match rx.recv_timeout(timeout) {
        Ok(l) => l,
        Err(RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Timeout { timeout });
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Protocol("进程在应答前退出".into()));
        }
    };
    let _ = child.kill();
    let _ = child.wait();

    serde_json::from_str::<Value>(line.trim())
        .map_err(|e| ToolError::Protocol(format!("解析应答失败: {e}")))
}

/// 发一次 `status` 并解析成报告。
///
/// 应答 `ok: false` 时返回 `Rejected`——把工具自己的 code/stage/消息原样带出去，
/// 宿主不翻译（工具的诊断比宿主的猜测准）。
pub fn query_status(launcher: &Launcher, timeout: Duration) -> Result<StatusReport, ToolError> {
    let raw = request_once(
        launcher,
        &serde_json::json!({
            "id": 1,
            "cmd": "status",
            "clientProtocol": crate::proto::PROTOCOL_VERSION,
        }),
        timeout,
    )?;
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
}

/// 会话式召回的终态事件名。见到任意一个就收尾——**不能靠进程退出判断**
/// （正常收尾和崩溃退出长得一样，混为一谈会漏掉"半份结果"）。
/// 会话终态事件名。读到其中之一就收尾（关进程、放槽位、调 `on_exit`）。
///
/// `pub` 是为了让 `tests/proto_mirror.rs` 拿它与镜像 schema 里的 `type`
/// 常量集合比对——改名而忘了改这里，症状是"会话永远不结束"，很难现场定位。
pub const TERMINAL_EVENTS: &[&str] = &["done", "applyDone", "cancelled", "sessionFailed"];

/// 起会话转发线程：把每一行 JSON 转发给 `sink`，见终态事件后收尾。
///
/// `on_exit` 一定在"会话真正结束之后"调用（收尸完、槽位已释放），用于让
/// 调用方在数据落盘后做后续动作（比如对账）。它跑在自己的线程里，不阻塞读循环。
pub fn spawn_reader(
    mut child: Child,
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
        return Err(e);
    }

    // Arc<Mutex<Child>>：会话线程持有一份（kill/wait），失败路径也持有一份收尸
    let child = Arc::new(Mutex::new(child));
    let child_for_fail = Arc::clone(&child);

    std::thread::Builder::new()
        .name("tool-session-reader".into())
        .spawn(move || {
            let mut idle = SESSION_IDLE_TIMEOUT;
            loop {
                match line_rx.recv_timeout(idle) {
                    Ok(line) => {
                        idle = SESSION_IDLE_TIMEOUT; // 有输出就续命
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
                    Err(RecvTimeoutError::Disconnected) => break, // 进程退出
                }
            }
            // 关 stdin → 对面读到 EOF 后退出；wait 收尸防僵尸
            drop(release());
            if let Ok(mut c) = child.lock() {
                let _ = c.wait();
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
            drop(release());
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
    use std::sync::Mutex as StdMutex;

    /// 收集事件的测试出口。
    #[derive(Default)]
    struct CollectSink(StdMutex<Vec<(String, Value)>>);

    impl EventSink for CollectSink {
        fn emit(&self, channel: &'static str, payload: Value) {
            self.0.lock().unwrap().push((channel.to_string(), payload));
        }
    }

    #[test]
    fn reserve_release_round_trip() {
        let _g = test_lock();
        assert!(!flag().load(Ordering::SeqCst));
        assert!(reserve(), "空槽必须能占");
        assert!(!reserve(), "已占用时第二次必须失败");
        release();
        assert!(reserve(), "释放后必须能再占");
        release();
    }

    #[test]
    fn cancel_without_session_is_false() {
        let _g = test_lock();
        let _ = release();
        assert!(!cancel(), "没有会话时取消是幂等的 false");
    }

    /// 真起一个子进程验协议：用 `echo` 之类保证跨平台可用的命令不存在，
    /// 所以这里不 spawn，只验纯逻辑（真进程验证在 src-tauri 的 dev 测试里）。
    #[test]
    fn terminal_event_set_is_the_documented_four() {
        assert_eq!(TERMINAL_EVENTS, &["done", "applyDone", "cancelled", "sessionFailed"]);
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

    #[test]
    fn collect_sink_records_channel() {
        let sink = CollectSink::default();
        sink.emit("ch", serde_json::json!({"type": "x"}));
        let got = sink.0.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "ch");
    }
}
