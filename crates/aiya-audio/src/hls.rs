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
use std::time::Duration;

use url::Url;

use crate::decoder::{DecodeBackend, DecodedStream};
use crate::AudioError;

/// 段与 playlist 的拉取口。抽 trait 是为了单测注入 [`MockFetcher`]——
/// 调度状态机的所有行为（live 轮询 / 断网退避 / 段跳过）都不出网可测。
pub(crate) trait Fetcher {
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
}

#[derive(Debug)]
struct ParsedMedia {
    segments: Vec<RawSegment>,
    endlist: bool,
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
        if let Some(attr) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let _ = attr;
            next_is_variant_uri = true;
            continue;
        }
        if next_is_variant_uri && !line.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
}

/// 解析 media playlist。`base` 用于把相对 URI 解析成绝对地址。
fn parse_media_playlist(text: &str, base: &Url) -> Result<ParsedMedia, AudioError> {
    let mut segments = Vec::new();
    let mut pending_duration_ms: Option<u64> = None;
    let mut target_duration_ms = 10_000;
    let mut endlist = false;

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
        segments.push(RawSegment {
            uri: resolved,
            duration_ms: pending_duration_ms.take().unwrap_or(target_duration_ms),
        });
    }

    Ok(ParsedMedia { segments, endlist })
}

/* ---------------- 段调度器 ---------------- */

/// live 模式的轮询间隔。刻意小于 TARGETDURATION：engine 阻塞在
/// next_packet 里时不响应 Stop/NewLoad，500ms 把"按停止没反应"的
/// 最坏延迟压在 1 秒内。
const LIVE_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 段拉取重试次数（单段）。超过后 live 跳过该段（缺口），VOD 直接报错。
const SEGMENT_FETCH_RETRIES: u32 = 3;
/// 连续失败上限（playlist 或段级累计）：退避到顶还连不上就报错，
/// 引擎走错误恢复（error 事件 + 停在当前曲），UI 提示"直播中断"。
const MAX_CONSECUTIVE_FAILURES: u32 = 40;
/// 退避基数：第 n 次失败睡 min(500ms × n, 5s)
const BACKOFF_BASE: Duration = Duration::from_millis(500);
const BACKOFF_CAP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Vod,
    Live,
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
    /// 已消费过的段 URL（live 轮询去重；VOD 不需要但留着无害）
    played: HashSet<String>,
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
            .field("failures", &self.failures)
            .finish()
    }
}

impl HlsStream {
    /// 打开 HLS 流：拉 playlist →（master 则跟进第一个变体）→ 解析 →
    /// 立即打开第一段（engine 构造后马上要 sample_rate/channels 发 Loaded）。
    pub fn new(url: &str) -> Result<Self, AudioError> {
        Self::with_fetcher(url, Box::new(HttpFetcher::new()))
    }

    fn with_fetcher(url: &str, mut fetcher: Box<dyn Fetcher>) -> Result<Self, AudioError> {
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
            .map_err(|e| AudioError::Decode(format!("playlist 地址无效 {playlist_url:?}: {e}")))?;
        let parsed = parse_media_playlist(&text, &base)?;

        let kind = if parsed.endlist { Kind::Vod } else { Kind::Live };
        let duration_ms = if kind == Kind::Vod {
            parsed.segments.iter().map(|s| s.duration_ms).sum()
        } else {
            0
        };

        let mut pending: VecDeque<_> = parsed.segments.into_iter().collect();
        let mut played = HashSet::new();

        // 立即打开第一段：engine 拿到 DecodedStream 就要读 sample_rate/channels
        let first = pending.pop_front().ok_or_else(|| {
            AudioError::Decode("HLS playlist 里没有任何段".to_string())
        })?;
        played.insert(first.uri.clone());
        let current = Some(Box::new(Self::open_segment(&mut *fetcher, &first, None)?));
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

    /// 打开一段：拉字节 → probe（hint=aac，ADTS）→ 校验与首段参数一致。
    /// `expected` = Some((rate, ch)) 时跨段变化报流错误。
    fn open_segment(
        fetcher: &mut dyn Fetcher,
        seg: &RawSegment,
        expected: Option<(u32, usize)>,
    ) -> Result<DecodedStream, AudioError> {
        let bytes = Self::fetch_with_retry(fetcher, &seg.uri)?;
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
        let mut appended = false;
        for seg in parsed.segments {
            if self.played.insert(seg.uri.clone()) {
                self.pending.push_back(seg);
                appended = true;
            }
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

    /// 失败记账 + 退避。超过上限报错（断网 N 秒恢复由重连逻辑覆盖，
    /// 上限 40 次 × 最长 5s ≈ 数分钟量级的网络中断都能熬过去）。
    fn record_failure(&mut self) -> Result<(), AudioError> {
        self.failures += 1;
        if self.failures > MAX_CONSECUTIVE_FAILURES {
            return Err(AudioError::Io(std::io::Error::other(format!(
                "HLS 流中断：连续 {MAX_CONSECUTIVE_FAILURES} 次拉取失败（{}）",
                self.playlist_url
            ))));
        }
        let backoff = BACKOFF_BASE
            .saturating_mul(self.failures.min(10) as u32)
            .min(BACKOFF_CAP);
        self.fetcher.sleep(backoff);
        Ok(())
    }

    /// 下一包交错 f32（与 Symphonia 变体同语义：EOF = Ok(None)）。
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
                            Ok(_) => {
                                self.failures = 0;
                                if self.pending.is_empty() {
                                    // 没新段（正常轮询窗口）：小睡等下一段，不算失败
                                    self.fetcher.sleep(self.poll_interval);
                                }
                            }
                            Err(_) => self.record_failure()?,
                        }
                        continue;
                    }
                }
            }

            // 打开下一段
            let seg = self.pending.pop_front().unwrap();
            match Self::open_segment(
                &mut *self.fetcher,
                &seg,
                Some((self.sample_rate, self.channels)),
            ) {
                Ok(stream) => {
                    self.played.insert(seg.uri.clone());
                    self.current = Some(Box::new(stream));
                    self.failures = 0;
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
    struct MockFetcher {
        seq: std::collections::HashMap<String, VecDeque<SeqItem>>,
        bytes: std::collections::HashMap<String, Vec<u8>>,
        /// sleep 累计毫秒。Rc 共享：fetcher 会被 move 进 HlsStream，
        /// 测试侧留个句柄断言"退避真的发生了"
        sleeps_ms: std::rc::Rc<std::cell::Cell<u64>>,
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
                sleeps_ms: Default::default(),
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
            self.sleeps_ms.set(self.sleeps_ms.get() + d.as_millis() as u64);
        }
    }

    const BASE: &str = "http://fm.test/live/stream.m3u8";

    fn playlist(body: &str) -> String {
        format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n{body}")
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
        let s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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

    /// 断网放弃：playlist 连续失败超过上限 → 报"流中断"，退避真的发生。
    #[test]
    fn playlist_failures_give_up_after_limit() {
        let seg = adts_segment(4, 4, 2);
        // v0 之后 45 次 Fail（> MAX_CONSECUTIVE_FAILURES=40），Fail 不重复
        // 也无妨——放弃发生在 40 次
        let mut seq = vec![SeqItem::Text(playlist("#EXTINF:1.0,\ns1.aac"))];
        for _ in 0..45 {
            seq.push(SeqItem::Fail);
        }
        let fetcher = MockFetcher::new()
            .with_seq(BASE, seq)
            .with_segment("http://fm.test/live/s1.aac", seg);
        let sleeps = fetcher.sleeps_ms.clone();
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        let e = err.expect("连续失败必须最终报错");
        assert!(e.to_string().contains("连续"), "报错要说清是流中断：{e}");
        assert!(sleeps.get() > 0, "退避必须真的发生");
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        assert!(sleeps.get() > 0, "断网期间的退避必须发生");
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
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
        let mut s = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap();
        assert_eq!(s.sample_rate(), 44100);
        while let Ok(Some(_)) = s.current.as_mut().unwrap().next_packet() {}
        s.current = None;
        let err = s.next_packet().unwrap_err();
        assert!(
            err.to_string().contains("中途变化"),
            "参数变化要说清：{err}"
        );
    }

    /// 空 playlist（没有任何段）→ 打开阶段就报错。
    #[test]
    fn empty_playlist_is_error_on_open() {
        let fetcher = MockFetcher::new().with_text(BASE, &playlist("#EXT-X-ENDLIST"));
        let err = HlsStream::with_fetcher(BASE, Box::new(fetcher)).unwrap_err();
        assert!(err.to_string().contains("没有任何段"), "{err}");
    }
}
