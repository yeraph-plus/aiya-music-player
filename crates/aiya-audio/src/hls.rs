//! HLS (m3u8) 流式解码源 — docs/22 G3，路线 A：ADTS AAC packed-audio。
//!
//! 为什么是 ADTS 而不是 TS/fMP4：Symphonia 0.5 没有 TS demuxer、fMP4 要升
//! 0.6（live 泄漏 #586，docs/21 §4 冻结）；AAC 的 ADTS 裸流用现有 `aac`
//! feature 就能逐段 probe。Liquidsoap 的 FM 出流也定 adts（docs/22 §3）。
//!
//! 调度模型（engine 只认 [`crate::decoder::DecodedStream`]，这里是其中
//! 一个变体）：
//! * VOD（有 `EXT-X-ENDLIST`）：顺序拉段直到 EOF，总时长 = Σ EXTINF。
//! * live（无 ENDLIST）：段取空后回轮 playlist 追加新段；断网按失败计数
//!   退避重连，超过上限报 `AudioError::Io`（引擎按错误恢复流程处理）。
//! * 每段独立 probe（hint=aac）解码；**跨段采样率/声道变化 = 流错误**
//!   （ADTS 允许中途换参，但我们的输出流/WASAPI 已按首段开好，换参只能断）。
//!
//! 显式拒绝（docs/22 §4 明确不做，报 `Unsupported` 说人话）：
//! TS 段、AES-128/SAMPLE-AES 加密、`EXT-X-MAP`（fMP4 初始化段）。
//! master playlist（多 rendition）取第一个变体继续，tracing 记日志。
//!
//! 端到端验收随远程版 R2 FM soak（Liquidsoap 出真段）；这里单测锁
//! 解析矩阵 + 调度状态机（[`MockFetcher`] 注入，不出网）。

use std::collections::{HashSet, VecDeque};
use std::io::{Cursor, Read};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use url::Url;

use crate::decoder::{DecodeBackend, DecodedStream};
use crate::AudioError;

/// 段与 playlist 的拉取口。抽 trait 是为了单测注入 [`MockFetcher`]——
/// 调度状态机的所有行为（live 轮询 / 断网退避 / 段跳过）都不出网可测。
///
/// 为什么要求 `Send`：段字节由[`Prefetch`]线程拉，与音频线程并行。否则每个
/// 段边界都要在 `next_packet` 里同步等一次 HTTP——独占输出缓冲只有几十毫秒，
/// 每段结尾就会断一下。
pub(crate) trait Fetcher: Send {
    fn fetch_text(&mut self, url: &str) -> Result<String, AudioError>;
    fn fetch_bytes(&mut self, url: &str) -> Result<Vec<u8>, AudioError>;
    /// 失败后的退避睡眠（测试里替换成零耗时）
    fn sleep(&mut self, d: Duration);
}

/// 生产实现：ureq 同步阻塞。引擎线程本来就是阻塞模型（读文件也是阻塞），
/// 不为拉段引入异步运行时。
struct HttpFetcher {
    agent: ureq::Agent,
}

impl HttpFetcher {
    fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                // 单请求总超时：段大小通常 <1MB，15s 足够窄带；连接失败走
                // 调度器的退避重连，不在这里长等
                .timeout(Duration::from_secs(15))
                .build(),
        }
    }
}

fn http_err(url: &str, e: ureq::Error) -> AudioError {
    AudioError::Io(std::io::Error::other(format!("HTTP {url}: {e}")))
}

impl Fetcher for HttpFetcher {
    fn fetch_text(&mut self, url: &str) -> Result<String, AudioError> {
        let body = self.agent.get(url).call().map_err(|e| http_err(url, e))?;
        let mut bytes = Vec::new();
        body.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| AudioError::Io(std::io::Error::other(format!("读响应失败 {url}: {e}"))))?;
        // m3u8 规范要求 UTF-8；坏字节按 lossy 兜底（比整条流报错友好）
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn fetch_bytes(&mut self, url: &str) -> Result<Vec<u8>, AudioError> {
        let body = self.agent.get(url).call().map_err(|e| http_err(url, e))?;
        let mut bytes = Vec::new();
        body.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| AudioError::Io(std::io::Error::other(format!("读响应失败 {url}: {e}"))))?;
        Ok(bytes)
    }

    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/* ---------------- playlist 解析（纯函数） ---------------- */

#[derive(Debug, Clone)]
struct RawSegment {
    uri: String,
    duration_ms: u64,
    /// 绝对序号（`#EXT-X-MEDIA-SEQUENCE` + 段内下标）。没有该标签时为 None
    /// ——那是"窗口不滑动"的老式 playlist，只能靠 URI 去重。
    seq: Option<u64>,
}

#[derive(Debug)]
struct ParsedMedia {
    segments: Vec<RawSegment>,
    endlist: bool,
    /// `#EXT-X-TARGETDURATION`（毫秒），live「多久没新段算死」的基准
    target_duration_ms: u64,
}

/// 解析 media playlist。`base` 用于把相对 URI 解析成绝对地址。
fn parse_media_playlist(text: &str, base: &Url) -> Result<ParsedMedia, AudioError> {
    let mut segments = Vec::new();
    let mut pending_duration_ms: Option<u64> = None;
    let mut target_duration_ms = 10_000;
    let mut endlist = false;
    // 段序号基数：`#EXT-X-MEDIA-SEQUENCE` 的语义是"本 playlist 第一个段的
    // 绝对序号"。有了它才能在 URI 复用（滚动命名）时仍判断出新段。
    let mut media_sequence: Option<u64> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // EXTINF:<duration(s)>,[title] — 时长是小数秒
            let dur_str = rest.split(',').next().unwrap_or("").trim();
            let secs: f64 = dur_str
                .parse()
                .map_err(|_| AudioError::Decode(format!("EXTINF 时长不是数字：{dur_str:?}")))?;
            pending_duration_ms = Some((secs.max(0.0) * 1000.0).round() as u64);
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            target_duration_ms = rest.trim().parse().unwrap_or(10) * 1000;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = rest.trim().parse().ok();
            continue;
        }
        if line == "#EXT-X-ENDLIST" {
            endlist = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            // METHOD=NONE 才放行；其余（AES-128 / SAMPLE-AES）显式拒绝
            let method = rest
                .split(',')
                .find_map(|a| a.trim().strip_prefix("METHOD="))
                .unwrap_or("UNKNOWN");
            if !method.eq_ignore_ascii_case("NONE") {
                return Err(AudioError::Unsupported(format!(
                    "加密 HLS 不支持（METHOD={method}）"
                )));
            }
            continue;
        }
        if line.starts_with("#EXT-X-MAP:") {
            return Err(AudioError::Unsupported(
                "fMP4 初始化段（EXT-X-MAP）不支持：本播放器走 ADTS AAC 路线".into(),
            ));
        }
        if line.starts_with('#') {
            continue; // 其余标签（VERSION/DISCONTINUITY/PROGRAM-DATE-TIME…）忽略
        }

        // ---- URI 行 = 一个段 ----
        if line.to_ascii_lowercase().ends_with(".ts") {
            return Err(AudioError::Unsupported(
                "MPEG-TS 段不支持：本播放器只解 ADTS AAC 段（.aac / 无扩展名）".into(),
            ));
        }
        let resolved = base
            .join(line)
            .map_err(|e| AudioError::Decode(format!("段地址解析失败 {line:?}: {e}")))?
            .to_string();
        let index = segments.len() as u64;
        segments.push(RawSegment {
            uri: resolved,
            duration_ms: pending_duration_ms.take().unwrap_or(target_duration_ms),
            seq: media_sequence.map(|base| base + index),
        });
    }

    Ok(ParsedMedia { segments, endlist, target_duration_ms })
}

/// master playlist 的第一个变体 URI（有的话）。
/// 音频播放器没有带宽自适应的意义，取第一个（通常是最高码率）继续。
fn extract_master_variant(text: &str) -> Option<String> {
    let mut next_is_variant_uri = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.strip_prefix("#EXT-X-STREAM-INF:").is_some() {
            next_is_variant_uri = true;
            continue;
        }
        if next_is_variant_uri && !line.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/* ---------------- 段调度器 ---------------- */

/// live 模式的轮询间隔。engine 阻塞在 `next_packet` 里时不响应 Stop/NewLoad，
/// 所以这个间隔就是"按停止没反应"的**健康路径**延迟（生病路径见
/// [`OUTAGE_BUDGET`]）。
const LIVE_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 段拉取重试次数（单段）。超过后 live 跳过该段（缺口），VOD 直接报错。
const SEGMENT_FETCH_RETRIES: u32 = 3;
/// **一次网络中断的总预算**（墙钟，不是次数）。
///
/// 为什么不用"失败 N 次"：退避是同步睡在 `next_packet` 里的，次数上限换成
/// 时间是不可控的——按次数 40 次能睡出 3 分钟（叠加 HTTP 超时更长），这期间
/// 用户按暂停/停止毫无反应。改成一个明确的墙钟预算：熬过 20 秒还没恢复就
/// 认输报错（UI 亮"直播中断"），至少不会假死。
const OUTAGE_BUDGET: Duration = Duration::from_secs(20);
/// 退避基数：第 n 次失败睡 min(500ms × n, 5s)
const BACKOFF_BASE: Duration = Duration::from_millis(500);
const BACKOFF_CAP: Duration = Duration::from_secs(5);
/// live「多久没有新内容」算流已死（下限；实际上限按 3×TARGETDURATION 算）。
/// 没有这条，服务端一旦不再更新 playlist（或 URI 复用被判成旧段），就会
/// 500ms 一轮永远静默轮询下去：不报错、不 EOF、也没声音。
const STALL_FLOOR: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Vod,
    Live,
}

/// 段字节预取器：一个常驻线程，按请求拉取，结果回传。
///
/// 为什么必须并发：`next_packet` 是音频线程直接调的，engine 的样本池只垫
/// 一轮设备写入的量。段边界上同步 fetch 一次 HTTP，就等于在设备缓冲见底时
/// 才开始下载——共享模式 200ms 缓冲还能盖住几十毫秒 RTT，独占模式（10–50ms
/// 缓冲）每段必断音。预取让下载与播放并行，段边界通常零等待。
///
/// 只做**一段** lookahead（串行 worker + 单槽）：段序列是严格顺序的，多段
/// 预取只会浪费带宽，还要处理乱序与取消。
struct Prefetch {
    req_tx: Sender<String>,
    res_rx: Receiver<(String, Result<Vec<u8>, String>)>,
    /// 已经排进队列、还没收到结果的段
    inflight: Option<String>,
    /// 提前到达但暂时用不上的段（顺序正常时不会出现）
    cached: Option<(String, Vec<u8>)>,
}

impl Prefetch {
    fn new(mut fetcher: Box<dyn Fetcher + 'static>, label: &str) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<String>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<(String, Result<Vec<u8>, String>)>();
        // 线程名带 playlist 标识，卡住时能在调试器里认出来是哪个台
        let _ = std::thread::Builder::new()
            .name(format!("aiya-hls-prefetch {label}"))
            .spawn(move || {
                // req_tx 断（HlsStream 被 drop）= 退出。卡在 HTTP 里最多
                // 一个请求超时就走完这一轮再退，不影响正确性。
                while let Ok(url) = req_rx.recv() {
                    let r = fetcher.fetch_bytes(&url).map_err(|e| e.to_string());
                    if res_tx.send((url, r)).is_err() {
                        break; // 接收端没了
                    }
                }
            });
        Self { req_tx, res_rx, inflight: None, cached: None }
    }

    /// 请求预取某段。重复请求同一 URL 直接跳过（轮询会反复喊同一个下一段）。
    fn submit(&mut self, url: &str) {
        if self.inflight.as_deref() == Some(url)
            || self.cached.as_ref().is_some_and(|(u, _)| u == url)
        {
            return;
        }
        if self.req_tx.send(url.to_string()).is_ok() {
            self.inflight = Some(url.to_string());
        }
    }

    /// 把已完成的结果收进 `cached`/清掉 `inflight`。
    fn pump(&mut self) {
        while let Ok((url, result)) = self.res_rx.try_recv() {
            if self.inflight.as_deref() == Some(url.as_str()) {
                self.inflight = None;
            }
            match result {
                Ok(bytes) => self.cached = Some((url, bytes)),
                // 预取失败不在这里重试：同步兜底路径有重试与退避语义
                Err(e) => tracing::debug!("HLS 预取失败（{url}）：{e}"),
            }
        }
    }

    /// 取某段的字节。`None` = 没预取到（失败或还没到），调用方走同步兜底。
    fn take(&mut self, want: &str) -> Option<Vec<u8>> {
        self.pump();
        match self.cached.take() {
            Some((url, bytes)) if url == want => Some(bytes),
            other => {
                self.cached = other;
                None
            }
        }
    }

    /// 单测专用：等某段预取到货（否则"预取命中"的断言会随机成真成假）。
    #[cfg(test)]
    fn wait_ready(&mut self, want: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.pump();
            if self.cached.as_ref().is_some_and(|(u, _)| u == want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }
}

/// HLS 源。对外暴露与 Symphonia [`crate::decoder`] 相同的形状：
/// `sample_rate / channels / duration_ms / next_packet / position_ms / seekable`。
pub(crate) struct HlsStream {
    fetcher: Box<dyn Fetcher>,
    playlist_url: String,
    kind: Kind,
    /// 已确认的段（VOD：初始 playlist 全量；live：追加进来的待播段）
    pending: VecDeque<RawSegment>,
    /// 当前段的解码器。Box 断开 `DecodedStream → Inner::Hls → HlsStream
    /// → DecodedStream` 的无限大小递归。
    current: Option<Box<DecodedStream>>,
    /// 交错样本暂存：`next_packet` 返回它的切片。段内解码器的缓冲经
    /// `self.current` 借出，引用会把 `self.current` 冻结整个函数体
    /// （get_default 困境：段耗尽时要置换 current 就借不动了）——
    /// 所以每包转存到这里再借出，与 Symphonia 变体的 sample_buf 同模式。
    buf: Vec<f32>,
    sample_rate: u32,
    channels: usize,
    /// VOD = Σ EXTINF；live = 0（UI 按 source=stream 显示"直播中"）
    duration_ms: u64,
    frames_played: u64,
    /// 已消费过的段 URL（去重兜底：没有 `EXT-X-MEDIA-SEQUENCE` 的 playlist
    /// 只能靠 URI 判新旧）
    played: HashSet<String>,
    /// 已消费过的最大段序号（有 MEDIA-SEQUENCE 时才是判新旧的权威依据：
    /// 服务端滚动复用段名时 URI 会重复，但序号不会）
    last_seq: Option<u64>,
    /// 段字节预取（与音频线程并行拉下一段）
    prefetch: Prefetch,
    /// 目标段时长（毫秒），live 停滞判定的基准
    target_duration_ms: u64,
    /// 最近一次"有进展"的时刻（追加了新段 / 成功打开了段）
    last_progress: Instant,
    /// 停滞期里已"睡"掉的轮询总时长（与 `last_progress` 取大者，理由同
    /// [`Self::outage_elapsed`]：mock sleep 零成本，单测要能推进停滞判定）
    stall_slept: Duration,
    /// 本次网络中断的起点（None = 当前健康）
    outage_since: Option<Instant>,
    /// 本次中断里已经"睡"掉的退避总时长。与 [`Self::outage_since`] 取大者
    /// 作为预算消耗——理由见 [`Self::outage_elapsed`]
    outage_slept: Duration,
    failures: u32,
    /// 单测注入：轮询间隔
    poll_interval: Duration,
}

impl std::fmt::Debug for HlsStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // fetcher 是 trait object 不实现 Debug；调度状态足够定位问题
        f.debug_struct("HlsStream")
            .field("playlist_url", &self.playlist_url)
            .field("kind", &self.kind)
            .field("pending", &self.pending.len())
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("frames_played", &self.frames_played)
            .field("last_seq", &self.last_seq)
            .field("failures", &self.failures)
            .finish()
    }
}

impl HlsStream {
    /// 打开 HLS 流：拉 playlist →（master 则跟进第一个变体）→ 解析 →
    /// 立即打开第一段（engine 构造后马上要 sample_rate/channels 发 Loaded）。
    pub fn new(url: &str) -> Result<Self, AudioError> {
        Self::with_fetcher(url, || Box::new(HttpFetcher::new()))
    }

    /// `make_fetcher` 会被调用两次：同步调度一份、预取线程一份。测试里传
    /// 一个"克隆同一份 mock"的闭包即可（mock 的计数器用 Arc 共享）。
    fn with_fetcher(
        url: &str,
        mut make_fetcher: impl FnMut() -> Box<dyn Fetcher + 'static>,
    ) -> Result<Self, AudioError> {
        let mut fetcher = make_fetcher();
        let mut playlist_url = url.to_string();
        let mut text = fetcher.fetch_text(&playlist_url)?;

        // master playlist：跟进第一个变体（只跟一层；media playlist 不含
        // STREAM-INF，二跳无从发生）
        if let Some(variant) = extract_master_variant(&text) {
            let resolved = Url::parse(url)
                .map_err(|e| AudioError::Decode(format!("playlist 地址无效 {url:?}: {e}")))?
                .join(&variant)
                .map_err(|e| AudioError::Decode(format!("变体地址解析失败 {variant:?}: {e}")))?
                .to_string();
            tracing::info!("HLS master playlist → 跟进第一个变体：{resolved}");
            playlist_url = resolved;
            text = fetcher.fetch_text(&playlist_url)?;
        }

        let base = Url::parse(&playlist_url)
            .map_err(|e| AudioError::Decode(format!("playlist 地址无效 {playlist_url:?}：{e}")))?;
        let parsed = parse_media_playlist(&text, &base)?;

        let kind = if parsed.endlist { Kind::Vod } else { Kind::Live };
        let duration_ms = if kind == Kind::Vod {
            parsed.segments.iter().map(|s| s.duration_ms).sum()
        } else {
            0
        };
        // 序号基线取初始窗口的最大值：live 下一次轮询只看 seq 更大的段。
        let last_seq = parsed.segments.iter().filter_map(|s| s.seq).max();
        let target_duration_ms = parsed.target_duration_ms;

        let mut pending: VecDeque<_> = parsed.segments.into_iter().collect();
        let mut played = HashSet::new();
        let mut prefetch = Prefetch::new(make_fetcher(), &playlist_url);

        // 立即打开第一段：engine 拿到 DecodedStream 就要读 sample_rate/channels
        let first = pending.pop_front().ok_or_else(|| {
            AudioError::Decode("HLS playlist 里没有任何段".to_string())
        })?;
        played.insert(first.uri.clone());
        // 打开首段的同时就把第二段排进预取队列（首段本身就快，这里主要是
        // 让"打开"到"播到段尾"之间有一段完整的时间窗给下载用）
        if let Some(next) = pending.front() {
            prefetch.submit(&next.uri);
        }
        let current = Some(Box::new(Self::open_segment(&mut *fetcher, &mut prefetch, &first, None)?));
        let sample_rate = current.as_ref().unwrap().sample_rate();
        let channels = current.as_ref().unwrap().channels();

        Ok(Self {
            fetcher,
            playlist_url,
            kind,
            pending,
            current,
            buf: Vec::new(),
            sample_rate,
            channels,
            duration_ms,
            frames_played: 0,
            played,
            last_seq,
            prefetch,
            target_duration_ms,
            last_progress: Instant::now(),
            stall_slept: Duration::ZERO,
            outage_since: None,
            outage_slept: Duration::ZERO,
            failures: 0,
            poll_interval: LIVE_POLL_INTERVAL,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
    pub fn seekable(&self) -> bool {
        false
    }
    pub fn position_ms(&self) -> u64 {
        self.frames_played * 1000 / (self.sample_rate as u64).max(1)
    }

    /// 让预取线程去拉"下一个要播的段"（如果还没在拉/没到）。
    fn prime_prefetch(&mut self) {
        if let Some(next) = self.pending.front() {
            let uri = next.uri.clone();
            self.prefetch.submit(&uri);
        }
    }

    /// 打开一段：取字节（优先预取，兜底同步拉）→ probe（hint=aac，ADTS）→
    /// 校验与首段参数一致。`expected` = Some((rate, ch)) 时跨段变化报流错误。
    fn open_segment(
        fetcher: &mut dyn Fetcher,
        prefetch: &mut Prefetch,
        seg: &RawSegment,
        expected: Option<(u32, usize)>,
    ) -> Result<DecodedStream, AudioError> {
        // 预取命中 = 零网络等待；没命中（预取失败/还没到）走同步重试，
        // 这条路会真的阻塞音频线程，所以只留给异常情况。
        let bytes = match prefetch.take(&seg.uri) {
            Some(b) => b,
            None => Self::fetch_with_retry(fetcher, &seg.uri)?,
        };
        let mut hint = symphonia::core::probe::Hint::new();
        hint.with_extension("aac");
        let stream = DecodedStream::open_with_backend(
            Box::new(Cursor::new(bytes)),
            &hint,
            DecodeBackend::Symphonia,
        )
        .map_err(|e| {
            AudioError::Decode(format!("HLS 段解不开（{}）：{e}", seg.uri))
        })?;
        if let Some((rate, ch)) = expected {
            if stream.sample_rate() != rate || stream.channels() != ch {
                return Err(AudioError::Decode(format!(
                    "HLS 流参数中途变化：{}Hz×{} → {}Hz×{}（{}）",
                    rate,
                    ch,
                    stream.sample_rate(),
                    stream.channels(),
                    seg.uri
                )));
            }
        }
        Ok(stream)
    }

    /// 带重试的拉取。失败计退避睡眠；返回最终错误（调用方决定跳过或失败）。
    fn fetch_with_retry(fetcher: &mut dyn Fetcher, url: &str) -> Result<Vec<u8>, AudioError> {
        let mut last = None;
        for _ in 0..SEGMENT_FETCH_RETRIES {
            match fetcher.fetch_bytes(url) {
                Ok(b) => return Ok(b),
                Err(e) => {
                    last = Some(e);
                    fetcher.sleep(BACKOFF_BASE);
                }
            }
        }
        Err(last.unwrap_or_else(|| AudioError::Io(std::io::Error::other("fetch 失败"))))
    }

    /// live 轮询：重新拉 playlist，把没播过的新段追加到队尾。
    /// 全部失败返回 Err；无新段返回 Ok(false)。
    fn refresh(&mut self) -> Result<bool, AudioError> {
        let text = self.fetcher.fetch_text(&self.playlist_url)?;
        let base = Url::parse(&self.playlist_url).map_err(|e| {
            AudioError::Decode(format!("playlist 地址无效 {:?}: {e}", self.playlist_url))
        })?;
        let parsed = parse_media_playlist(&text, &base)?;
        self.target_duration_ms = parsed.target_duration_ms;
        let mut appended = false;
        for seg in parsed.segments {
            // 判新旧：有 MEDIA-SEQUENCE 就比序号（**URI 可能被服务端滚动复用**，
            // 只比 URI 会把新段当成旧段 → 永远不会再有内容，静默卡死）；
            // 没序号（老式不滑动的 playlist）才退回 URI 去重。
            let is_new = match (seg.seq, self.last_seq) {
                (Some(s), Some(last)) => s > last,
                _ => !self.played.contains(&seg.uri),
            };
            if !is_new {
                continue;
            }
            if let Some(s) = seg.seq {
                self.last_seq = Some(self.last_seq.map_or(s, |last| last.max(s)));
            }
            self.played.insert(seg.uri.clone());
            self.pending.push_back(seg);
            appended = true;
        }
        if appended {
            self.mark_progress();
            self.prime_prefetch();
        }
        // live 播着播着出现 ENDLIST（活动结束转回放）：段取空后按 VOD 收尾
        if parsed.endlist {
            self.kind = Kind::Vod;
            if self.duration_ms == 0 {
                self.duration_ms = self.position_ms()
                    + self.pending.iter().map(|s| s.duration_ms).sum::<u64>();
            }
        }
        Ok(appended)
    }

    /// 失败记账 + 退避，**总时长受 [`OUTAGE_BUDGET`] 约束**。
    ///
    /// 这里的 sleep 是同步睡在 `next_packet` 里的：睡多久，engine 就有多久
    /// 处理不了 Pause/Stop/Next。所以上限必须按时间算（按次数算出来的是
    /// 分钟级假死），到点认输报错，让 UI 亮出来。
    fn record_failure(&mut self) -> Result<(), AudioError> {
        self.failures += 1;
        self.outage_since.get_or_insert_with(Instant::now);
        let elapsed = self.outage_elapsed();
        if elapsed >= OUTAGE_BUDGET {
            return Err(AudioError::Io(std::io::Error::other(format!(
                "HLS 流中断：{} 秒内拉取一直失败（{}）",
                OUTAGE_BUDGET.as_secs(),
                self.playlist_url
            ))));
        }
        // 退避按失败次数爬坡，但**不超过剩余预算**：最后一段睡眠不能越过死线，
        // 否则预算又变成了"最多 +5s"的软约束。
        let backoff = BACKOFF_BASE
            .saturating_mul(self.failures.min(10) as u32)
            .min(BACKOFF_CAP)
            .min(OUTAGE_BUDGET - elapsed);
        self.fetcher.sleep(backoff);
        self.outage_slept += backoff;
        Ok(())
    }

    /// 本次中断已消耗的时间：真实流逝与"已睡掉的退避"取大者。
    ///
    /// 为什么要两者取大：生产里 HTTP 请求超时是真的在流逝（15s 一次），
    /// 单看 sleep 会低估；而单测的 mock sleep 是零成本的（只计数不真睡），
    /// 单看真实流逝会永远到不了预算、退避路径就没法测。取大者两边都对。
    fn outage_elapsed(&self) -> Duration {
        let real = self.outage_since.map(|s| s.elapsed()).unwrap_or_default();
        real.max(self.outage_slept)
    }

    /// 本次中断结束（拉到东西了）：预算清零。
    fn clear_outage(&mut self) {
        self.failures = 0;
        self.outage_since = None;
        self.outage_slept = Duration::ZERO;
    }

    /// live 停滞判定：多久没有新内容就把流判死。
    fn stall_budget(&self) -> Duration {
        Duration::from_millis(self.target_duration_ms.saturating_mul(3)).max(STALL_FLOOR)
    }

    fn stall_elapsed(&self) -> Duration {
        self.last_progress.elapsed().max(self.stall_slept)
    }

    /// 记一次"有进展"：重置停滞判定（真实时刻与虚拟睡眠都要归零，否则
    /// 单测里一次睡眠就把后续进度全算成停滞了）。
    fn mark_progress(&mut self) {
        self.last_progress = Instant::now();
        self.stall_slept = Duration::ZERO;
    }

    /// 下一包交错 f32（与 Symphonia 变体同语义：EOF = Ok(None)）。
    ///
    /// 借用结构刻意拆成两阶段：`advance()` 负责**全部**可变调度（换段/
    /// 轮询/退避），返回后只在 `current` 上借一次包——返回的切片借着
    /// `self.current`，所以拉包路径上绝不能再碰 `self` 的其他写操作。
    pub fn next_packet(&mut self) -> Result<Option<&[f32]>, AudioError> {
        loop {
            if !self.advance()? {
                return Ok(None); // VOD 播完
            }
            // 从当前段拉一包、转存到自有 buf（转存路径不外借引用，
            // 段内借用出块即结束，段耗尽时才能置换 current）。
            let exhausted = {
                let seg = self.current.as_mut().expect("advance 保证 current 就绪");
                match seg.next_packet()? {
                    Some(samples) => {
                        self.buf.clear();
                        self.buf.extend_from_slice(samples);
                        // 位置 = 已产出帧数累计（含当前包）。Symphonia 变体用
                        // 包时间戳，ADTS 段没有连续时间轴，帧计数是唯一时钟。
                        let frames = (self.buf.len() / self.channels.max(1)) as u64;
                        self.frames_played += frames;
                        false
                    }
                    None => true,
                }
            };
            if exhausted {
                self.current = None; // 段耗尽 → 下一轮 advance 开下一段
                continue;
            }
            // 切片在 return 语句现场创建、立即离开函数，
            // 其后没有任何代码——借用检查与 Symphonia 变体同构。
            return Ok(Some(&self.buf));
        }
    }

    /// 调度阶段：推进到 `current` 就绪（返回 true）或 VOD 播完（false）。
    fn advance(&mut self) -> Result<bool, AudioError> {
        loop {
            if self.current.is_some() {
                return Ok(true);
            }
            if self.pending.is_empty() {
                match self.kind {
                    Kind::Vod => return Ok(false), // 初始就是 VOD：playlist 全量播完
                    Kind::Live => {
                        match self.refresh() {
                            Ok(appended) => {
                                self.clear_outage();
                                if !appended && self.pending.is_empty() {
                                    // 没有新段：可能是正常轮询窗口，也可能是服务端
                                    // 已经不更新了（甚至 URI 复用被判成旧段）。
                                    // 前者小睡，后者必须报错——否则就是永远静默轮询。
                                    if self.stall_elapsed() >= self.stall_budget() {
                                        return Err(AudioError::Io(std::io::Error::other(format!(
                                            "HLS 直播流已停止更新：{} 秒没有新段（{}）",
                                            self.stall_budget().as_secs(),
                                            self.playlist_url
                                        ))));
                                    }
                                    self.fetcher.sleep(self.poll_interval);
                                    self.stall_slept += self.poll_interval;
                                }
                            }
                            Err(_) => self.record_failure()?,
                        }
                        continue;
                    }
                }
            }

            // 先把**当前段 pop 掉**，再让预取去排队"下一个段"（顺序反了就会
            // 把正要同步打开的段也预取一遍，白下一份）。
            let seg = self.pending.pop_front().unwrap();
            self.prime_prefetch();
            match Self::open_segment(
                &mut *self.fetcher,
                &mut self.prefetch,
                &seg,
                Some((self.sample_rate, self.channels)),
            ) {
                Ok(stream) => {
                    self.played.insert(seg.uri.clone());
                    if let Some(s) = seg.seq {
                        self.last_seq = Some(self.last_seq.map_or(s, |last| last.max(s)));
                    }
                    self.current = Some(Box::new(stream));
                    self.mark_progress();
                    self.clear_outage();
                }
                Err(e) => match self.kind {
                    // VOD 段坏 = 流坏
                    Kind::Vod => return Err(e),
                    // live 段坏：跳过缺口继续（下一段重新对参数——参数错仍会报）
                    Kind::Live => {
                        tracing::warn!("HLS live 段跳过（{}）：{e}", seg.uri);
                        self.record_failure()?;
                    }
                },
            }
        }
    }
}

/* ---------------- 单测：解析矩阵 + 调度状态机（mock，不出网） ---------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    /// 手拼最小 ADTS 段：N 帧合法 ADTS 头 + 垃圾负载。
    /// probe/next_packet 都认头；负载解不出 → 坏包跳过 → 段 EOF。
    /// 这就是"没有 ffmpeg 造不出真 AAC"下的务实替身（docs/22 §5）。
    /// 位域按 ISO 14496-3 ADTS 规范逐字节拼，freq_idx: 4=44100, 3=48000。
    fn adts_segment(frames: usize, freq_idx: u8, channels: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let frame_len: u16 = 7 + 32;
        for _ in 0..frames {
            // B0: sync 0xFF；B1: 0xF1 = sync | ID=MPEG-4 | layer=0 | protection_absent=1
            // B2: profile=LC(01) <<6 | freq_idx <<2 | private=0 | ch 高位
            // B3: ch 低 2 位 <<6 | 原始/版权 4 位=0 | 帧长高 2 位
            // B4-5: 帧长低 11 位；B5-6: fullness=0x7FF | frames-1=0
            let mut h = [0u8; 7];
            h[0] = 0xFF;
            h[1] = 0xF1;
            h[2] = 0x40 | (freq_idx << 2) | ((channels >> 2) & 1);
            h[3] = ((channels & 3) << 6) as u8 | ((frame_len >> 11) & 3) as u8;
            h[4] = ((frame_len >> 3) & 0xFF) as u8;
            h[5] = (((frame_len & 7) << 5) | ((0x7FF >> 6) & 0x1F)) as u8;
            h[6] = (((0x7FF & 0x3F) << 2) | 0) as u8;
            out.extend_from_slice(&h);
            // 全 0 负载 = AAC 元素 id 000（SCE）+ 全零 ICS → 解码出静音帧。
            // 别用 0xAB 之类：前 3 位凑成 101 = PCE，symphonia 直接报
            // Unsupported（fatal），调度测试就被解码器挡住了。
            out.extend_from_slice(&[0x00; 32]);
        }
        out
    }

    /// 每个URL的响应序列：依次弹出，**最后一个 Text 无限重复**（live 轮询
    /// 会一直拉同一份 playlist），Fail 项不重复（弹掉即消耗）。
    /// 序列耗尽 = 404。这个语义正好表达"电台 playlist 会演化"。
    ///
    /// 可 Clone：同步调度与预取线程各拿一份（预取只用 `bytes`，不碰 `seq`
    /// 游标，所以克隆快照没有语义问题；计数器用 Arc 共享，两边的 sleep 都记进
    /// 同一个总数）。
    #[derive(Clone)]
    struct MockFetcher {
        seq: std::collections::HashMap<String, VecDeque<SeqItem>>,
        bytes: std::collections::HashMap<String, Vec<u8>>,
        /// sleep 累计毫秒。Arc 共享：fetcher 会被 move 进 HlsStream（还有
        /// 预取线程），测试侧留个句柄断言"退避真的发生了"
        sleeps_ms: Arc<AtomicU64>,
    }

    #[derive(Clone)]
    enum SeqItem {
        Text(String),
        Fail,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                seq: Default::default(),
                bytes: Default::default(),
                sleeps_ms: Arc::default(),
            }
        }
        /// 设置 URL 的响应序列
        fn with_seq(mut self, url: &str, items: Vec<SeqItem>) -> Self {
            self.seq.insert(url.into(), items.into());
            self
        }
        fn with_text(self, url: &str, text: &str) -> Self {
            self.with_seq(url, vec![SeqItem::Text(text.into())])
        }
        fn with_segment(mut self, url: &str, bytes: Vec<u8>) -> Self {
            self.bytes.insert(url.into(), bytes);
            self
        }
    }

    impl Fetcher for MockFetcher {
        fn fetch_text(&mut self, url: &str) -> Result<String, AudioError> {
            let q = self
                .seq
                .get_mut(url)
                .ok_or_else(|| AudioError::Io(std::io::Error::other(format!("404 {url}"))))?;
            let item = if q.len() <= 1 {
                // 剩最后一项：Text 无限重复，Fail 也重复（反正调用方会放弃）
                q.front().cloned().unwrap()
            } else {
                q.pop_front().unwrap()
            };
            match item {
                SeqItem::Text(t) => Ok(t),
                SeqItem::Fail => Err(AudioError::Io(std::io::Error::other("mock 断网"))),
            }
        }
        fn fetch_bytes(&mut self, url: &str) -> Result<Vec<u8>, AudioError> {
            self.bytes
                .get(url)
                .cloned()
                .ok_or_else(|| AudioError::Io(std::io::Error::other(format!("404 {url}"))))
        }
        fn sleep(&mut self, d: Duration) {
            self.sleeps_ms
                .fetch_add(d.as_millis() as u64, Ordering::Relaxed);
        }
    }

    const BASE: &str = "http://fm.test/live/stream.m3u8";

    fn playlist(body: &str) -> String {
        format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n{body}")
    }

    /// 用同一份 mock 造流：同步调度与预取线程各取一个克隆。
    fn try_open(fetcher: MockFetcher) -> Result<HlsStream, AudioError> {
        HlsStream::with_fetcher(BASE, move || Box::new(fetcher.clone()))
    }

    fn open(fetcher: MockFetcher) -> HlsStream {
        try_open(fetcher).expect("流应能打开")
    }

    /* ---- 解析矩阵 ---- */

    #[test]
    fn vod_playlist_parses_with_durations_and_endlist() {
        let base = Url::parse(BASE).unwrap();
        let p = parse_media_playlist(
            &playlist("#EXTINF:3.5,\na/seg1.aac\n#EXTINF:2.25,Artist - Title\nseg2.aac\n#EXT-X-ENDLIST"),
            &base,
        )
        .unwrap();
        assert!(p.endlist);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.segments[0].uri, "http://fm.test/live/a/seg1.aac");
        assert_eq!(p.segments[0].duration_ms, 3500);
        assert_eq!(p.segments[1].uri, "http://fm.test/live/seg2.aac");
        assert_eq!(p.segments[1].duration_ms, 2250);
    }

    #[test]
    fn relative_urls_resolve_against_playlist_dir() {
        let base = Url::parse(BASE).unwrap();
        let p = parse_media_playlist(
            &playlist("#EXTINF:1.0,\n../other/seg.aac\nhttp://cdn.test/x.aac"),
            &base,
        )
        .unwrap();
        assert_eq!(p.segments[0].uri, "http://fm.test/other/seg.aac");
        assert_eq!(p.segments[1].uri, "http://cdn.test/x.aac");
    }

    #[test]
    fn aes128_key_is_explicitly_rejected() {
        let base = Url::parse(BASE).unwrap();
        let err = parse_media_playlist(
            &playlist("#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:1.0,\ns.aac"),
            &base,
        )
        .unwrap_err();
        assert!(matches!(err, AudioError::Unsupported(ref m) if m.contains("AES-128")), "{err}");
    }

    #[test]
    fn method_none_key_is_allowed() {
        let base = Url::parse(BASE).unwrap();
        let p = parse_media_playlist(
            &playlist("#EXT-X-KEY:METHOD=NONE\n#EXTINF:1.0,\ns.aac"),
            &base,
        )
        .unwrap();
        assert_eq!(p.segments.len(), 1);
    }

    #[test]
    fn ext_x_map_is_explicitly_rejected() {
        let base = Url::parse(BASE).unwrap();
        let err = parse_media_playlist(
            &playlist("#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\ns.m4s"),
            &base,
        )
        .unwrap_err();
        assert!(matches!(err, AudioError::Unsupported(ref m) if m.contains("EXT-X-MAP")), "{err}");
    }

    #[test]
    fn ts_segment_is_explicitly_rejected() {
        let base = Url::parse(BASE).unwrap();
        let err = parse_media_playlist(
            &playlist("#EXTINF:1.0,\nseg0.ts"),
            &base,
        )
        .unwrap_err();
        assert!(matches!(err, AudioError::Unsupported(ref m) if m.contains("MPEG-TS")), "{err}");
    }

    #[test]
    fn master_playlist_takes_first_variant() {
        let v = extract_master_variant(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nhi/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=64000\nlo/index.m3u8",
        )
        .unwrap();
        assert_eq!(v, "hi/index.m3u8");
        assert!(extract_master_variant(&playlist("#EXTINF:1.0,\ns.aac")).is_none());
    }

    #[test]
    fn no_endlist_means_live() {
        let fetcher = MockFetcher::new()
            .with_text(BASE, &playlist("#EXTINF:1.0,\ns1.aac"))
            .with_segment("http://fm.test/live/s1.aac", adts_segment(4, 4, 2));
        let s = open(fetcher);
        assert_eq!(s.kind, Kind::Live);
        assert_eq!(s.duration_ms(), 0);
        assert!(!s.seekable());
    }

    /* ---- 调度状态机 ---- */

    /// 两段 VOD（合法 ADTS 头 + 垃圾负载）：应顺序消费两段后 EOF，
    /// 采样率来自 ADTS 头（index 4 = 44100Hz，双声道）。
    #[test]
    fn vod_two_segments_then_eof() {
        let seg = adts_segment(8, 4, 2);
        let fetcher = MockFetcher::new()
            .with_text(
                BASE,
                &playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXT-X-ENDLIST"),
            )
            .with_segment("http://fm.test/live/s1.aac", seg.clone())
            .with_segment("http://fm.test/live/s2.aac", seg);
        let mut s = open(fetcher);
        assert_eq!(s.sample_rate(), 44100);
        assert_eq!(s.channels(), 2);
        assert_eq!(s.duration_ms(), 2000);

        // 头 8 帧垃圾负载：解不出样本 → 两段消费完直接 EOF，不 panic
        let mut packets = 0;
        while let Ok(Some(samples)) = s.next_packet() {
            assert!(!samples.is_empty());
            packets += 1;
        }
        assert_eq!(packets, 0, "垃圾负载解不出样本，但两段调度必须走完且 EOF");
        assert_eq!(s.position_ms(), 0);
    }

    /// live：三轮 playlist 演化（1 段 → +s2 → +s3+ENDLIST）。
    /// 段全部消费完、ENDLIST 出现 → EOF（活动结束转回放收尾）。
    #[test]
    fn live_polls_new_segments_until_endlist() {
        let seg = adts_segment(4, 4, 2);
        let v1 = playlist("#EXTINF:1.0,\ns1.aac");
        let v2 = playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac");
        let v3 = playlist(
            "#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXTINF:1.0,\ns3.aac\n#EXT-X-ENDLIST",
        );
        let fetcher = MockFetcher::new()
            .with_seq(
                BASE,
                vec![
                    SeqItem::Text(v1),
                    SeqItem::Text(v2),
                    SeqItem::Text(v3), // 最后一项无限重复（此后 kind 已转 VOD，不再 refresh）
                ],
            )
            .with_segment("http://fm.test/live/s1.aac", seg.clone())
            .with_segment("http://fm.test/live/s2.aac", seg.clone())
            .with_segment("http://fm.test/live/s3.aac", seg);
        let mut s = open(fetcher);
        s.poll_interval = Duration::from_millis(1);
        assert_eq!(s.kind, Kind::Live);

        // 垃圾负载解不出样本，纯驱动调度：三段走完必须自然 EOF
        let mut eof = false;
        for _ in 0..1000 {
            match s.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => panic!("三段健全的流不该报错：{e}"),
            }
        }
        assert!(eof, "ENDLIST + 段耗尽必须 EOF（不能无限轮询）");
        assert_eq!(s.kind, Kind::Vod, "轮询中见到 ENDLIST 要转 VOD");
        // 时长 = 已播帧数 + 待播段 EXTINF 总和（合成段的帧数与 EXTINF
        // 声明不一致是预期的——mock 段只有 4×1024 样本 ≈ 93ms）
        assert!(s.duration_ms() > 0, "ENDLIST 后时长要补齐");
    }

    /// 断网放弃：预算耗尽就报"流中断"，且退避真的发生。
    ///
    /// mock 的 sleep 是零成本的（只累加计数），所以「预算」靠
    /// `outage_elapsed()` 里的虚拟时长推进——这也正是它取 max 的原因。
    #[test]
    fn playlist_outage_gives_up_after_budget() {
        let seg = adts_segment(4, 4, 2);
        // v0 之后一直是 Fail（最后一项无限重复）
        let mut seq = vec![SeqItem::Text(playlist("#EXTINF:1.0,\ns1.aac"))];
        for _ in 0..45 {
            seq.push(SeqItem::Fail);
        }
        let fetcher = MockFetcher::new()
            .with_seq(BASE, seq)
            .with_segment("http://fm.test/live/s1.aac", seg);
        let sleeps = fetcher.sleeps_ms.clone();
        let mut s = open(fetcher);
        s.poll_interval = Duration::from_millis(1);

        while let Ok(Some(_)) = s.current.as_mut().unwrap().next_packet() {}
        s.current = None;
        let mut err = None;
        for _ in 0..200 {
            match s.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("live 流不该 EOF"),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        let e = err.expect("预算耗尽必须最终报错");
        assert!(e.to_string().contains("流中断"), "报错要说清是流中断：{e}");
        assert!(
            sleeps.load(Ordering::Relaxed) >= OUTAGE_BUDGET.as_millis() as u64 / 2,
            "退避必须真的发生（且不超过预算）：{}ms",
            sleeps.load(Ordering::Relaxed)
        );
        // 预算内不许超过死线：累计睡眠不得超过 OUTAGE_BUDGET（末次睡眠被裁剪）
        assert!(
            sleeps.load(Ordering::Relaxed) <= OUTAGE_BUDGET.as_millis() as u64 + 1_000,
            "睡眠总量失控：{}ms",
            sleeps.load(Ordering::Relaxed)
        );
    }

    /// 断网恢复：playlist 失败几次后恢复出新段，调度继续并正常收尾。
    #[test]
    fn playlist_failures_then_recover() {
        let seg = adts_segment(4, 4, 2);
        let v1 = playlist("#EXTINF:1.0,\ns1.aac");
        let v2 = playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXT-X-ENDLIST");
        let fetcher = MockFetcher::new()
            .with_seq(
                BASE,
                vec![SeqItem::Text(v1), SeqItem::Fail, SeqItem::Fail, SeqItem::Text(v2)],
            )
            .with_segment("http://fm.test/live/s1.aac", seg.clone())
            .with_segment("http://fm.test/live/s2.aac", seg);
        let sleeps = fetcher.sleeps_ms.clone();
        let mut s = open(fetcher);
        s.poll_interval = Duration::from_millis(1);

        let mut eof = false;
        for _ in 0..1000 {
            match s.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => panic!("断网恢复后不该报错：{e}"),
            }
        }
        assert!(eof, "恢复出 ENDLIST 后必须正常收尾");
        assert!(sleeps.load(Ordering::Relaxed) > 0, "断网期间的退避必须发生");
    }

    /// live 缺段（一个段 404）：跳过缺口继续播下一段，最终正常收尾。
    #[test]
    fn live_skips_missing_segment() {
        let seg = adts_segment(4, 4, 2);
        let v1 = playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac");
        let v2 = playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns3.aac\n#EXT-X-ENDLIST");
        let fetcher = MockFetcher::new()
            .with_seq(BASE, vec![SeqItem::Text(v1), SeqItem::Text(v2)])
            // s2.aac 不给 → 拉取失败 → live 跳过缺口
            .with_segment("http://fm.test/live/s1.aac", seg.clone())
            .with_segment("http://fm.test/live/s3.aac", seg);
        let mut s = open(fetcher);
        s.poll_interval = Duration::from_millis(1);

        let mut eof = false;
        for _ in 0..1000 {
            match s.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(e) => panic!("缺口跳过后不该报错：{e}"),
            }
        }
        assert!(eof, "跳过缺口 + ENDLIST 必须正常 EOF");
    }

    /// VOD 缺段：直接报错，不跳过（回放文件坏了就该让用户知道）。
    #[test]
    fn vod_missing_segment_is_fatal() {
        let seg = adts_segment(4, 4, 2);
        let fetcher = MockFetcher::new()
            .with_text(
                BASE,
                &playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXT-X-ENDLIST"),
            )
            .with_segment("http://fm.test/live/s1.aac", seg);
        let mut s = open(fetcher);
        while let Ok(Some(_)) = s.current.as_mut().unwrap().next_packet() {}
        s.current = None;
        let err = s.next_packet().unwrap_err();
        assert!(matches!(err, AudioError::Io(_)), "VOD 缺段报 Io：{err}");
    }

    /// 跨段采样率不一致 = 流错误（ADTS 允许，我们的输出链不允许）。
    #[test]
    fn cross_segment_sample_rate_change_is_fatal() {
        let seg44 = adts_segment(4, 4, 2); // 44100
        let seg48 = adts_segment(4, 3, 2); // 48000（index 3）
        let fetcher = MockFetcher::new()
            .with_text(
                BASE,
                &playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXT-X-ENDLIST"),
            )
            .with_segment("http://fm.test/live/s1.aac", seg44)
            .with_segment("http://fm.test/live/s2.aac", seg48);
        let mut s = open(fetcher);
        assert_eq!(s.sample_rate(), 44100);
        while let Ok(Some(_)) = s.current.as_mut().unwrap().next_packet() {}
        s.current = None;
        let err = s.next_packet().unwrap_err();
        assert!(
            err.to_string().contains("中途变化"),
            "参数变化要说清：{err}"
        );
    }

    /// 预取真的把下一段接过去了：同步侧故意不给 s2 的字节（给了就是 404），
    /// 只有预取命中才能播到 EOF。这条测的是"段边界零网络等待"这个属性本身，
    /// 没有它，独占模式下每段结尾都会断一下。
    #[test]
    fn prefetch_serves_next_segment_without_sync_fetch() {
        let seg = adts_segment(4, 4, 2);
        let s1 = "http://fm.test/live/s1.aac";
        let s2 = "http://fm.test/live/s2.aac";
        let text = playlist("#EXTINF:1.0,\ns1.aac\n#EXTINF:1.0,\ns2.aac\n#EXT-X-ENDLIST");

        // 同步侧：有 playlist、只有 s1（若 s2 走同步 → 404 → VOD 直接报错）
        let sync_side = MockFetcher::new().with_text(BASE, &text).with_segment(s1, seg.clone());
        // 预取侧：只有段字节（预取线程不拉 playlist，seq 里没东西也无所谓）
        let prefetch_side = MockFetcher::new().with_segment(s2, seg);
        let mut nth = 0;
        let mut s = HlsStream::with_fetcher(BASE, move || {
            nth += 1;
            // 第 1 次 = 同步调度，第 2 次 = 预取线程（见 with_fetcher 注释）
            if nth == 1 { Box::new(sync_side.clone()) } else { Box::new(prefetch_side.clone()) }
        })
        .unwrap();

        assert!(s.prefetch.wait_ready(s2), "预取线程没把 s2 拉回来");
        let mut eof = false;
        for _ in 0..200 {
            match s.next_packet() {
                Ok(None) => {
                    eof = true;
                    break;
                }
                Ok(Some(_)) => {}
                Err(e) => panic!("预取命中时不该走同步 404：{e}"),
            }
        }
        assert!(eof, "两段应顺序播完");
    }

    /// 空 playlist（没有任何段）→ 打开阶段就报错。
    #[test]
    fn empty_playlist_is_error_on_open() {
        let fetcher = MockFetcher::new().with_text(BASE, &playlist("#EXT-X-ENDLIST"));
        let err = try_open(fetcher).unwrap_err();
        assert!(err.to_string().contains("没有任何段"), "{err}");
    }

    /// URI 复用（滚动段名）：服务端每一轮都用同一个 `seg.aac`，只有
    /// MEDIA-SEQUENCE 在涨。只比 URI 会把新段全判成旧的 → 永远没有内容，
    /// 500ms 一轮静默轮询。这里锁住"按序号判新旧"。
    #[test]
    fn media_sequence_detects_new_segments_with_reused_uri() {
        let seg = adts_segment(4, 4, 2);
        // 三轮：seq=5 一段 → seq=6 一段 → seq=7 一段 + ENDLIST
        let v = |seq: u64, endlist: bool| {
            format!(
                "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{seq}\n#EXTINF:1.0,\nseg.aac\n{}",
                if endlist { "#EXT-X-ENDLIST" } else { "" }
            )
        };
        let fetcher = MockFetcher::new()
            .with_seq(
                BASE,
                vec![
                    SeqItem::Text(v(5, false)),
                    SeqItem::Text(v(6, false)),
                    SeqItem::Text(v(7, true)),
                ],
            )
            .with_segment("http://fm.test/live/seg.aac", seg);
        let mut s = open(fetcher);
        s.poll_interval = Duration::from_millis(1);

        let mut eof = false;
        for _ in 0..500 {
            match s.next_packet() {
                Ok(None) => {
                    eof = true;
                    break;
                }
                Ok(Some(_)) => {}
                Err(e) => panic!("URI 复用不该报错：{e}"),
            }
        }
        assert!(eof, "序号递增的三段必须被认成新段，最后 ENDLIST 收尾");
        assert_eq!(s.last_seq, Some(7), "基线要跟到最新序号");
    }

    /// 停滞判死：playlist 起来之后再也不更新（也没有 ENDLIST），
    /// 必须报"停止更新"，而不是永远 500ms 静默轮询。
    #[test]
    fn stalled_live_stream_gives_up() {
        // 只有一段，且 playlist 永远不变（最后一项无限重复）
        let fetcher = MockFetcher::new()
            .with_text(BASE, &playlist("#EXTINF:1.0,\ns1.aac"))
            .with_segment("http://fm.test/live/s1.aac", adts_segment(4, 4, 2));
        let mut s = open(fetcher);
        // 轮询间隔放大到 1s：停滞预算 15s 只需十几轮就能到（mock sleep 零成本，
        // 靠累加虚拟睡眠推进判定）
        s.poll_interval = Duration::from_secs(1);

        while let Ok(Some(_)) = s.current.as_mut().unwrap().next_packet() {}
        s.current = None;
        let mut err = None;
        for _ in 0..100 {
            match s.next_packet() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("live 流不该 EOF"),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        let e = err.expect("停滞的直播流必须报错而不是永远轮询");
        assert!(e.to_string().contains("停止更新"), "报错要说清是停滞：{e}");
    }
}
