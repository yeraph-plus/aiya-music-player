//! 播放状态类型：对齐 `@aiya/contracts/player.ts`。
//!
//! 状态里不放队列本身（5 万首 ID 数组不进每秒快照）；
//! 队列按版本号单独拉取（queueVersion 变化才重取）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackStatus {
    Idle,
    Loading,
    Playing,
    Paused,
    Error,
}

/// 音频规格（播放条徽章/规格显示；对齐 contracts 的 AudioFormat）。
/// 列表查询不填（None），只有播放条数据源（单曲级查询）会带。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackAudio {
    pub codec: Option<String>,
    pub container: Option<String>,
    #[ts(type = "number | null")]
    pub sample_rate: Option<i64>,
    #[ts(type = "number | null")]
    pub bits_per_sample: Option<i64>,
    /// **单位 kbps**（不是 bps）。两条来源一致：lofty 的 `audio_bitrate()`
    /// 文档写 `(kbps)`，beets 的 `bitrate` 字段文档写 "in kilobits per second"。
    /// 展示时直接取整即可，别再除 1000（前端曾这么干，320 kbps 的 MP3 显示成 0）。
    #[ts(type = "number | null")]
    pub bitrate: Option<i64>,
    #[ts(type = "number | null")]
    pub channels: Option<i64>,
    /// 三态：`Some(true)` 无损 / `Some(false)` 有损 / `None` 认不出来。
    ///
    /// UI 的规格文字、Hi-Res 高亮、tooltip 的「无损/有损」**只看这个字段**，
    /// 不看 `bits_per_sample`——有损编码也会报 16 bit（那是解码器输出位深），
    /// 按位深分支会把 m4a 标成「44.1 kHz / 16 bit / 无损」。
    ///
    /// 由 [`TrackAudio::infer_lossless`] 在出口派生，不落库（理由见该函数）。
    /// 2026-10-25 补：Tauri 重写时整个字段丢了，导致播放条徽章恒为「未知」。
    pub lossless: Option<bool>,
}

/// 按 codec / container 判断是否无损时用到的关键词表。
///
/// 为什么是 `contains` 而不是精确相等：两边来源的取值都很脏——
/// 扫描器写 lofty 的 `FileType` Debug 名（`Flac` / `Mpeg` / `Wav`），
/// beets 写 `format`（`FLAC` / `MP3` / `ALAC`），标签里还可能冒出来
/// `MPEG 1 Layer 3`、`AAC (Advanced Audio Coding)` 这类长串。
///
/// 顺序要求：`aac` 必须在 `mpeg` 前判过（`MPEG-4 AAC` 同时含两个词，
/// 但 AAC 有损、MPEG Layer 3 也有损，结论一致，故此处不敏感）；
/// 真正敏感的是 `mp4`（ALAC 无损 / AAC 有损都可能），所以它**不在**这两张表里。
const LOSSLESS_HINTS: &[&str] = &[
    "flac",
    "alac",
    "pcm",
    "linear",
    "aiff",
    "aif",
    "wav",   // 同时覆盖 lofty 的 `Wav` 与标签里的 `WAVE`
    "dsd",
    "dsf",
    "dff",
    "wavpack",
    "monkey", // Monkey's Audio
    "ape",    // 同上：lofty 的 FileType::Ape 就写作 `Ape`，无其它 codec 名含此串
    "tta",
    "shorten",
];

const LOSSY_HINTS: &[&str] = &[
    "mp3",
    "mpeg",
    "layer 3",
    "layer iii",
    "aac",
    "opus",
    "vorbis",
    "wma",
    "windows media",
    "atrac",
    "musepack",
    "mpc",
    "speex",
    "ac-3",
    "eac3",
    "amr",
];

impl TrackAudio {
    /// 按 codec / container 派生「是否无损」。认不出返回 `None`。
    ///
    /// **为什么不存一列 `lossless`**：它是 `codec`/`container` 的纯函数，
    /// 加列等于让同一事实有两个来源（beets 改 codec 后列不跟着变，或者反过来），
    /// 还要 schema 迁移 + 老库回填 + 扫描器/beets 两条写路径都记得维护它。
    /// 派生则永远和 codec 一致，代价是每次查询算一次字符串匹配（微秒级）。
    ///
    /// **为什么必须保守**：用户在拿这个字段判断音质。误判 `true` 会把 AAC
    /// 标成「无损 / 44.1 kHz / 16 bit」，比老实显示「未知」更糟。所以
    /// - codec 认得出 → 按 codec 判；
    /// - codec 缺失/不认识 → 只有「容器本身只承载无损编码」时才敢说 `true`；
    /// - `Mp4`/`M4a` 这种同时装着 ALAC 与 AAC 的容器 → `None`，宁可不说。
    pub fn infer_lossless(codec: Option<&str>, container: Option<&str>) -> Option<bool> {
        fn hit(haystack: &str, hints: &[&str]) -> bool {
            hints.iter().any(|h| haystack.contains(h))
        }

        let codec = codec.unwrap_or_default().trim().to_ascii_lowercase();
        if !codec.is_empty() {
            if hit(&codec, LOSSLESS_HINTS) {
                return Some(true);
            }
            if hit(&codec, LOSSY_HINTS) {
                return Some(false);
            }
        }

        let container = container.unwrap_or_default().trim().to_ascii_lowercase();
        if container.is_empty() {
            return None;
        }
        // 容器兜底：扫描器不填 codec（`container` 才存 lofty 的 FileType），
        // 所以这条分支是「只跑过扫描、还没跑 beets」时的**主路径**。
        if hit(&container, LOSSLESS_HINTS) {
            return Some(true);
        }
        if hit(&container, LOSSY_HINTS) {
            return Some(false);
        }
        // Mp4 / M4a / Ogg 之类：容器底下无损有损都有，不猜
        None
    }
}

/// 队列元素：Track 的最小子集（**不含 path**，path 只在宿主内部流转）。
/// 宿主层从 aiya-data TrackRow 转换而来。
///
/// **camelCase 是契约要求**：`current` 直接作为 PlayerState.current 下发，
/// UI 读 `artworkId` / `gainDb` / `audio` —— snake_case 会让封面/规格/归一化全部静默失效。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub artwork_id: Option<String>,
    pub gain_db: Option<f64>,
    #[serde(default)]
    pub audio: Option<TrackAudio>,
    /// 引擎装载用；**不下发**（DTO 侧由 trackId 查询）。
    ///
    /// 曾经只是注释这么写、代码照发：`#[serde(default)]` 会把它一起序列化，
    /// UI 能从 `player:state.current.path` 拿到绝对路径。现在真的 skip。
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub path: String,
}

/// 播放失败的分类。**给 UI 用**：决定"自动跳下一首"还是"提示/亮灰标"。
///
/// 为什么不用字符串：v1 的 `error: Option<String>` 只有一句话，UI 无法判断
/// "文件没了（跳过就行）"和"声卡拔了（必须提示）"，只能靠人肉看文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum AudioErrorCode {
    /// 文件不存在/被删
    FileMissing,
    /// 其他 IO：权限、占用、磁盘
    Io,
    /// 解码失败：编码不支持、文件损坏
    Decode,
    /// 输出设备：打不开、断开、不消费数据
    DeviceLost,
    /// 设备/格式不支持（如"必须独占"被拒）
    UnsupportedFormat,
    /// 未分类
    Unknown,
}

impl AudioErrorCode {
    /// wire 值 → 枚举。与 `aiya_audio::AudioErrorKind::as_str()` 一一对应；
    /// 未知值按 `Unknown`（新引擎给新分类时，旧宿主不该直接崩）。
    pub fn parse(s: &str) -> Self {
        match s {
            "fileMissing" => AudioErrorCode::FileMissing,
            "io" => AudioErrorCode::Io,
            "decode" => AudioErrorCode::Decode,
            "deviceLost" => AudioErrorCode::DeviceLost,
            "unsupportedFormat" => AudioErrorCode::UnsupportedFormat,
            _ => AudioErrorCode::Unknown,
        }
    }
}

/// 播放失败：分类 + 一句话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AudioError {
    pub code: AudioErrorCode,
    pub message: String,
}

impl AudioError {
    pub fn new(code: AudioErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// 播放状态快照（对齐 PlayerState 契约形状，camelCase serde）。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub queue_length: usize,
    #[ts(type = "number")]
    pub queue_version: u64,
    /// 当前曲目在播放顺序（order）中的下标；空队列 = `null`。
    ///
    /// 为什么不用 -1 哨兵（docs/17 §9.2 偏差 1 的偿还）：`null` 在 TS 侧
    /// 无法与合法下标 0 混淆，`-1` 可以（漏判 `>= 0` 就是越界写）。缺值
    /// 一律 `null` 是本契约的既定规则，index/cursor 是最后两个例外。
    #[ts(type = "number | null")]
    pub index: Option<usize>,
    /// 播放顺序位置（随机模式下与 index 不同）；空队列 = `null`。
    #[ts(type = "number | null")]
    pub cursor: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub status: PlaybackStatus,
    #[ts(type = "number")]
    pub position_ms: u64,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub error: Option<AudioError>,
    pub current: Option<TrackRef>,
    #[ts(type = "number")]
    pub token: u64,
    /// 快照生成时间，**毫秒**。名字带 `Ms` 是硬要求：同结构里
    /// `positionMs`/`durationMs` 都是毫秒，只让它一个叫 `updatedAt`
    /// 等于留个坑给下一个人猜单位（docs/17 §3.1）。
    #[ts(type = "number")]
    pub updated_at_ms: u64,
}

/// 队列快照（对齐 QueueSnapshot：ids + tracks 最小字段 + 游标）。
/// v1 只给 ids + index；tracks 全量字段由 library_tracks 查询补齐。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub ids: Vec<String>,
    pub index: usize,
    pub cursor: usize,
    #[ts(type = "number")]
    pub version: u64,
    pub took_ms: u32,
}

/// 队列变更操作（对齐 UI 侧 QueueInsertMode 语义）。
#[derive(Debug, Clone)]
pub enum QueueOp {
    /// 替换整个队列并从 offset 播放
    ReplaceAll { tracks: Vec<TrackRef>, start: usize },
    /// 追加到队尾
    Append { tracks: Vec<TrackRef> },
    /// "下一首播放"
    PlayNext { tracks: Vec<TrackRef> },
    /// 移除指定位置（播放顺序 order 里的下标，与 queue_snapshot().ids 一致）
    Remove { indices: Vec<usize> },
    /// 重排：把 from 位置的曲目移到 to 位置（同一坐标系）
    Move { from: usize, to: usize },
}

/// PlayerActor 接受的命令。
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    TogglePause,
    Next,
    Prev,
    Seek(u64),
    SetVolume { volume: f64, muted: bool },
    SetRepeat(RepeatMode),
    SetRate(f64),
    ToggleShuffle,
    Queue(QueueOp),
    /// 引擎回报：装载完成（含总时长）
    Loaded { token: u64, duration_ms: u64 },
    /// 引擎回报：位置推进
    Position { position_ms: u64 },
    /// 引擎回报：曲目自然播完
    Ended { token: u64 },
    /// 引擎回报：错误
    Failed { error: AudioError },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 快照形状锚点：`current` 是 UI 播放条唯一数据源，
    /// 必须 camelCase（artworkId/gainDb/audio），否则封面/规格/归一化静默失效。
    #[test]
    fn current_serializes_camel_case() {
        let st = PlayerState {
            queue_length: 1,
            queue_version: 1,
            index: Some(0),
            cursor: Some(0),
            repeat: RepeatMode::Off,
            shuffle: false,
            status: PlaybackStatus::Playing,
            position_ms: 1000,
            duration_ms: Some(5000),
            volume: 0.8,
            muted: false,
            rate: 1.0,
            error: None,
            current: Some(TrackRef {
                id: "t1".into(),
                title: "曲".into(),
                artist: Some("人".into()),
                album: None,
                duration_ms: Some(5000),
                artwork_id: Some("aw_x".into()),
                gain_db: Some(-3.0),
                audio: Some(TrackAudio {
                    codec: Some("flac".into()),
                    container: None,
                    sample_rate: Some(48000),
                    bits_per_sample: Some(24),
                    bitrate: None,
                    channels: Some(2),
                    lossless: Some(true),
                }),
                path: "D:/m/a.flac".into(),
            }),
            token: 1,
            updated_at_ms: 0,
        };
        let v = serde_json::to_value(&st).unwrap();
        let cur = &v["current"];
        assert_eq!(cur["artworkId"], "aw_x", "封面字段必须 camelCase");
        assert_eq!(cur["gainDb"], -3.0, "归一化字段必须 camelCase");
        assert_eq!(cur["durationMs"], 5000);
        assert_eq!(cur["audio"]["sampleRate"], 48000, "规格字段必须 camelCase");
        assert_eq!(cur["audio"]["bitsPerSample"], 24);
        // 顶层快照字段本来就 camelCase，锚定防回归
        assert!(v.get("positionMs").is_some());
        assert!(v.get("durationMs").is_some());
        assert!(v.get("queueLength").is_some());
    }

    /// `lossless` 必须真的出现在 wire 上。
    ///
    /// 这条断言存在的理由：2026-10-25 之前 `TrackAudio` 根本没有这个字段，
    /// 而 TS 侧把它当存在（先是必填、后来被改成可选），于是播放条的
    /// 无损/有损徽章恒显示「未知」，跨了一个多月没人发现——因为**没有任何
    /// 测试在检查 payload 里有这个键**。形状断言比修 bug 本身更重要。
    #[test]
    fn audio_payload_carries_lossless() {
        let audio = TrackAudio {
            codec: Some("flac".into()),
            container: Some("Flac".into()),
            sample_rate: Some(44100),
            bits_per_sample: Some(16),
            bitrate: Some(900_000),
            channels: Some(2),
            lossless: TrackAudio::infer_lossless(Some("flac"), Some("Flac")),
        };
        let v = serde_json::to_value(&audio).unwrap();
        assert_eq!(v["lossless"], true, "lossless 必须下发，否则 UI 只能显示「未知」");
        assert_eq!(v["bitsPerSample"], 16);
    }

    /// 派生表：同一份数据来自两条写路径，取值风格完全不同，必须都能认。
    ///
    /// - 「只扫描」：`codec` 恒 NULL，`container` 是 lofty 的 `FileType` Debug 名
    ///   （`Flac` / `Mpeg` / `Wav` / `Mp4`）→ 兜底分支才是这类用户的主路径
    /// - 「跑过 beets」：`codec` 是 beets 的 `format`（`FLAC` / `MP3` / `ALAC`），
    ///   可能带标签里的长名（`MPEG 1 Layer 3`）
    #[test]
    fn infer_lossless_covers_both_write_paths() {
        let f = TrackAudio::infer_lossless;

        // --- 只扫描（codec=NULL，container=lofty FileType）---
        assert_eq!(f(None, Some("Flac")), Some(true), "FLAC 文件");
        assert_eq!(f(None, Some("Wav")), Some(true), "WAV 文件");
        assert_eq!(f(None, Some("Aiff")), Some(true), "AIFF 文件");
        assert_eq!(f(None, Some("Dsf")), Some(true), "DSD 文件");
        assert_eq!(f(None, Some("WavPack")), Some(true), "WavPack 文件");
        assert_eq!(f(None, Some("Mpeg")), Some(false), "lofty 对 MP3 报 Mpeg");
        assert_eq!(f(None, Some("Aac")), Some(false), "裸 AAC 文件");
        assert_eq!(f(None, Some("Opus")), Some(false), "lofty 已把 Opus 从 Ogg 里分出");
        assert_eq!(f(None, Some("Vorbis")), Some(false));
        assert_eq!(f(None, Some("Mpc")), Some(false), "Musepack 是有损");

        // --- Mp4 必须拒答：ALAC 无损 / AAC 有损共用这个容器 ---
        assert_eq!(f(None, Some("Mp4")), None, "Mp4 里可能是 ALAC 也可能是 AAC，不猜");

        // --- 跑过 beets（codec=beets format）---
        assert_eq!(f(Some("FLAC"), None), Some(true));
        assert_eq!(f(Some("ALAC"), None), Some(true), "ALAC 靠 codec 才能判，容器只说 Mp4");
        assert_eq!(f(Some("MP3"), None), Some(false));
        assert_eq!(f(Some("AAC"), None), Some(false));
        assert_eq!(f(Some("MPEG 1 Layer 3"), None), Some(false), "标签里的长名");
        assert_eq!(f(Some("PCM"), Some("Mpeg")), Some(true), "codec 优先于容器");

        // --- 认不出就 None，不许猜 ---
        assert_eq!(f(None, None), None);
        assert_eq!(f(Some(""), Some("  ")), None, "空白串等于没有");
        assert_eq!(f(Some("SomeNewCodec2027"), Some("Ogg")), None);
    }
}
