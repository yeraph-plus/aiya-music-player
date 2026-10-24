//! 歌词的形状。
//!
//! `aiya_data::lyrics::ParsedLyrics` 是解析器的内部结果（`encoding` 是 `String`，
//! 因为解析成功就一定有编码），IPC 侧需要的是"没有歌词"这一态——那时
//! `encoding` 为 null。所以这里另立一个 DTO，并在 `From` 里做**穷尽解构**：
//! 解析器将来加字段，这段会编译失败，强制有人想清楚它该不该进契约。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// 相对歌曲开头的毫秒（LRC `[offset:]` 已折进这里）
    #[ts(type = "number")]
    pub time_ms: i64,
    pub text: String,
}

/// 歌词。`found = false` 时 `lines` 为空数组，其余字段给零值——
/// UI 不需要为"没有歌词"写第二套分支（`lines.length === 0` 即可）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub found: bool,
    /// 按时间升序；无时间轴的纯文本歌词 `synced = false`
    pub lines: Vec<LyricLine>,
    /// LRC `[offset:]` 标签原值（诊断展示）；已折进各行 `timeMs`
    #[ts(type = "number")]
    pub offset_ms: i64,
    pub synced: bool,
    /// 检测到的文本编码（utf-8 / gbk …）；无歌词为 null
    pub encoding: Option<String>,
}

impl Lyrics {
    /// "没有歌词"：不是错误，UI 显示"暂无"。
    pub fn empty() -> Self {
        Self { found: false, lines: Vec::new(), offset_ms: 0, synced: false, encoding: None }
    }
}

impl From<aiya_data::lyrics::ParsedLyrics> for Lyrics {
    fn from(p: aiya_data::lyrics::ParsedLyrics) -> Self {
        // 穷尽解构：`ParsedLyrics` 加字段时这里编译失败（见模块注释）
        let aiya_data::lyrics::ParsedLyrics { found, lines, offset_ms, synced, encoding } = p;
        Self {
            found,
            lines: lines
                .into_iter()
                .map(|l| {
                    // 同样穷尽：`LyricLine` 加字段时编译失败
                    let aiya_data::lyrics::LyricLine { time_ms, text } = l;
                    LyricLine { time_ms, text }
                })
                .collect(),
            offset_ms,
            synced,
            encoding: Some(encoding),
        }
    }
}
