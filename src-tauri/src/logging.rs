//! 日志落盘：宿主日志与 sidecar 日志都留痕。
//!
//! 为什么必须有：此前全仓库没有 `tracing_subscriber` 初始化，`tracing::warn!`
//! 在 release 里**写进虚空**——`windows_subsystem = "windows"` 没有控制台，
//! 用户报"放不出声/起不来"时只能靠口述猜。sidecar 的 stderr 还被
//! `Stdio::null()` 丢掉，引擎进程崩溃同样无痕。
//!
//! 落点：`{app_data}/logs/aiya-YYYY-MM-DD.log`（按天分文件，保留 7 天），
//! sidecar 的 stderr 进同目录 `sidecar.log`。
//!
//! 级别：`AIYA_LOG`（如 `AIYA_LOG=aiya=debug,info`），其次 `RUST_LOG`，默认 `info`。
//! dev 构建额外输出到 stderr；release 只写文件（没有控制台可写）。
//!
//! 只用 `time` 取本地时间，不用 `tracing-appender`：滚动逻辑只有几十行，
//! 自己写能把"每次事件整行一次写入"这件事钉死（避免多线程行交错），
//! 也少一个依赖。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::layer::SubscriberExt;

/// 保留天数（按文件名日期排序，删最旧的）
const KEEP_DAYS: usize = 7;
/// sidecar 日志超过这个大小就在下次启动时换一份（.1）
const SIDECAR_ROTATE_BYTES: u64 = 8 * 1024 * 1024;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static INITED: OnceLock<()> = OnceLock::new();

/// 日志目录（`init` 之后可用）。sidecar stderr 重定向也用它。
pub fn dir() -> Option<&'static Path> {
    LOG_DIR.get().map(PathBuf::as_path)
}

fn local_now() -> time::OffsetDateTime {
    // Windows 上 now_local 走 GetTimeZoneInformation，没有 unix 那种 soundness 限制
    time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc())
}

fn date_of(t: time::OffsetDateTime) -> String {
    format!("{:04}-{:02}-{:02}", t.year(), u8::from(t.month()), t.day())
}

fn file_name(date: &str) -> String {
    format!("aiya-{date}.log")
}

/// 今天的日期（本地时区 YYYY-MM-DD）。诊断页要显示当天日志文件名。
pub fn today() -> String {
    date_of(local_now())
}

/// tracing 的时间戳（本地时间，毫秒）。
struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(
        &self,
        w: &mut tracing_subscriber::fmt::format::Writer<'_>,
    ) -> std::fmt::Result {
        let t = local_now();
        write!(
            w,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
            t.millisecond()
        )
    }
}

struct Inner {
    date: String,
    file: Option<File>,
}

/// 按天滚动的 append-only 文件。
struct LogFile {
    dir: PathBuf,
    inner: Mutex<Inner>,
}

impl LogFile {
    fn new(dir: PathBuf) -> Self {
        let date = date_of(local_now());
        let file = open_append(&dir, &date);
        let this = Self {
            dir,
            inner: Mutex::new(Inner { date, file }),
        };
        this.prune();
        this
    }

    /// 整行一次写入：多线程（事件泵 / 命令 / beets reader）并发时不会交错。
    fn write_bytes(&self, buf: &[u8]) {
        let today = date_of(local_now());
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.date != today || g.file.is_none() {
            // 跨日（或首次打开失败后重试）
            if let Some(f) = open_append(&self.dir, &today) {
                g.date = today;
                g.file = Some(f);
                drop(g);
                self.prune();
                g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
        if let Some(f) = g.file.as_mut() {
            // flush 每行一次：info 级日志量很小（一次会话几百行），
            // 换来的是进程被强杀时最后几行不丢——恰恰是最值钱的那几行。
            let _ = f.write_all(buf).and_then(|()| f.flush());
        }
    }

    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let mut ours: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let stem = name
                    .strip_prefix("aiya-")
                    .and_then(|s| s.strip_suffix(".log"))?;
                // 只认 aiya-YYYY-MM-DD.log，sidecar.log 之类不参与
                let ok = stem.len() == 10
                    && stem.as_bytes()[4] == b'-'
                    && stem.as_bytes()[7] == b'-';
                ok.then_some((stem.to_string(), e.path()))
            })
            .collect();
        if ours.len() <= KEEP_DAYS {
            return;
        }
        // 日期字符串可直接字典序比较
        ours.sort();
        let drop_n = ours.len() - KEEP_DAYS;
        for (_, path) in ours.into_iter().take(drop_n) {
            let _ = fs::remove_file(path);
        }
    }
}

fn open_append(dir: &Path, date: &str) -> Option<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file_name(date)))
        .ok()
}

struct SinkWriter<'a> {
    file: &'a LogFile,
    buf: Vec<u8>,
}

impl Write for SinkWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.file.write_bytes(&self.buf);
            self.buf.clear();
        }
        Ok(())
    }
}

// fmt 层写完一条事件后 drop writer，此时才真正落盘（一次 write，一行完整）
impl Drop for SinkWriter<'_> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogFile {
    type Writer = SinkWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SinkWriter {
            file: self,
            buf: Vec::with_capacity(256),
        }
    }
}

fn filter_from_env() -> tracing_subscriber::EnvFilter {
    use tracing_subscriber::EnvFilter;
    EnvFilter::try_from_env("AIYA_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"))
}

/// 初始化日志。幂等；任何失败都不影响启动（最差情况是只剩 stderr）。
pub fn init(app: &tauri::AppHandle) {
    if INITED.set(()).is_err() {
        return;
    }

    // 数据根目录由 paths 统一裁决（便携版在程序目录旁）；日志跟着走，
    // 否则"设置里说数据在 D 盘、日志却在 C 盘"。
    let dir = crate::paths::data_root(app).join("logs");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("[aiya] 日志目录创建失败 {}: {e}", dir.display());
        return;
    }
    let _ = LOG_DIR.set(dir.clone());

    rotate_sidecar_log(&dir);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(LogFile::new(dir.clone()))
        .with_ansi(false)
        .with_timer(LocalTime)
        .with_thread_names(true);

    let subscriber = tracing_subscriber::registry()
        .with(filter_from_env())
        .with(file_layer);

    // 开发态额外打到 stderr；release 是 windows 子系统、没有控制台
    #[cfg(debug_assertions)]
    let subscriber = subscriber.with(
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_timer(LocalTime)
            .with_thread_names(true),
    );

    use tracing_subscriber::util::SubscriberInitExt;
    if subscriber.try_init().is_err() {
        return;
    }

    // panic 也要留痕：release 崩溃时没有任何输出
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "panic", "{info}");
        prev(info);
    }));

    tracing::info!(
        target: "aiya",
        version = env!("CARGO_PKG_VERSION"),
        exe = %std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
        log_dir = %dir.display(),
        filter = %std::env::var("AIYA_LOG").or_else(|_| std::env::var("RUST_LOG")).unwrap_or_else(|_| "info".into()),
        "启动"
    );
}

/// sidecar 日志单文件无上限会越滚越大；启动时转一份 .1（够用，不做多份）
fn rotate_sidecar_log(dir: &Path) {
    let path = dir.join("sidecar.log");
    if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > SIDECAR_ROTATE_BYTES {
        let _ = fs::remove_file(dir.join("sidecar.log.1"));
        let _ = fs::rename(&path, dir.join("sidecar.log.1"));
    }
}

/// sidecar 的 stderr 出口：同目录 sidecar.log（拿不到日志目录就丢弃）。
pub fn sidecar_stderr() -> std::process::Stdio {
    let Some(dir) = dir() else {
        return std::process::Stdio::null();
    };
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("sidecar.log"))
    {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => std::process::Stdio::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "aiya-log-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn writes_whole_lines_and_reopens_after_missing_file() {
        let dir = tmp_dir("write");
        let f = LogFile::new(dir.clone());
        f.write_bytes(b"2026-10-24T06:00:00.000Z  INFO first\n");
        f.write_bytes(b"2026-10-24T06:00:01.000Z  WARN second\n");
        let today = today();
        let text = fs::read_to_string(dir.join(file_name(&today))).unwrap();
        assert_eq!(text.lines().count(), 2, "两行都要落盘：{text:?}");
        assert!(text.contains("first") && text.contains("second"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prunes_to_keep_days_but_leaves_foreign_files() {
        let dir = tmp_dir("prune");
        // 造 KEEP_DAYS+2 份历史 + 一份不归我们管的 sidecar.log
        for day in 1..=(KEEP_DAYS + 2) {
            fs::write(dir.join(format!("aiya-2026-09-{day:02}.log")), b"old").unwrap();
        }
        fs::write(dir.join("sidecar.log"), b"not ours").unwrap();
        let _f = LogFile::new(dir.clone());
        let ours = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("aiya-"))
            .count();
        assert_eq!(ours, KEEP_DAYS, "只保留 KEEP_DAYS 份");
        assert!(
            dir.join("sidecar.log").exists(),
            "别人的日志不能被 prune 误删"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn timestamp_is_local_and_sortable() {
        // 只要格式稳定到能被日志分析工具切分即可（YYYY-MM-DDTHH:MM:SS.mmm）
        let s = date_of(local_now());
        assert_eq!(s.len(), 10);
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
    }
}
