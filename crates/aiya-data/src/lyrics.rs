//! 歌词读取与 LRC 解析（P4）。
//!
//! 范围（docs/05 §7）：读文件 / 编码 / 解析。滚动与点击在 UI（lyrics-clock）。
//!
//! 查找策略：与音频文件同目录、同主文件名的 `.lrc`（Windows 文件系统大小写
//! 不敏感，直接拼即可）。v1 不做全网搜歌词、不读内嵌歌词（demo 也只有 .lrc）。
//!
//! 编码：BOM（UTF-8/UTF-16LE/BE）→ 严格 UTF-8 → GBK 兜底（中文场景最常见
//! 的历史遗留编码）。返回实际使用的编码名给诊断页展示。
//!
//! 解析：`[mm:ss.xx]` 一行多时间戳（展开成多行）；`[offset:+/-ms]` 全局校正
//! 已折进 timeMs；无时间戳行收集为纯文本（synced=false）。元数据标签
//! `[ti:][ar:][al:][by:][length:]` 忽略。时间升序稳定排序（同时间保持文件序）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLyrics {
    pub lines: Vec<LyricLine>,
    /// LRC `[offset:]` 标签原值（诊断展示）；已折进各行 timeMs
    pub offset_ms: i64,
    pub synced: bool,
    pub encoding: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LyricLine {
    pub time_ms: i64,
    pub text: String,
}

/// 读歌词主入口：`audio_path` 同目录找同名 `.lrc`；找不到返回 None。
pub fn read_lyrics(audio_path: &std::path::Path) -> Option<ParsedLyrics> {
    let lrc = lrc_path_for(audio_path)?;
    let bytes = std::fs::read(&lrc).ok()?;
    Some(parse_lrc(&bytes))
}

/// 同目录同名 .lrc 的路径。主名不重复扩展名（`a.b.flac` → `a.b.lrc` 里的
/// `a.b` 是完整主名；rsplit 一次即可）。
fn lrc_path_for(audio_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let file_name = audio_path.file_name()?.to_str()?;
    let stem = match file_name.rsplit_once('.') {
        Some((stem, _ext)) if !stem.is_empty() => stem.to_string(),
        _ => file_name.to_string(),
    };
    Some(audio_path.with_file_name(format!("{stem}.lrc")))
}

/// 解析 LRC 字节（含编码检测）。
pub fn parse_lrc(bytes: &[u8]) -> ParsedLyrics {
    let (text, encoding) = decode(bytes);
    let (mut timed, mut plain, offset_ms) = parse_lines(&text);

    // 有时间轴 → 只保留时间行（纯文本行通常是混入的空行/标题）；
    // 全无时间轴 → 纯文本歌词
    let synced = !timed.is_empty();
    if synced {
        timed.sort_by_key(|l| l.time_ms); // 稳定排序：同时间保持文件序
        ParsedLyrics { lines: timed, offset_ms, synced: true, encoding }
    } else {
        plain.retain(|l| !l.text.trim().is_empty());
        ParsedLyrics { lines: plain, offset_ms, synced: false, encoding }
    }
}

/// BOM → 严格 UTF-8 → GBK。
fn decode(bytes: &[u8]) -> (String, String) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(&bytes[3..]).into_owned(), "UTF-8".into());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (decode_utf16(&bytes[2..], true), "UTF-16LE".into());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (decode_utf16(&bytes[2..], false), "UTF-16BE".into());
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), "UTF-8".into()),
        Err(_) => {
            let (cow, _had_errors, _enc) = encoding_rs::GBK.decode(bytes);
            (cow.into_owned(), "GBK".into())
        }
    }
}

fn decode_utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if little {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// 行解析：返回 (时间行, 纯文本行, offset_ms)。
fn parse_lines(text: &str) -> (Vec<LyricLine>, Vec<LyricLine>, i64) {
    let mut timed = Vec::new();
    let mut plain = Vec::new();
    let mut offset_ms = 0i64;

    for raw in text.lines() {
        let line = raw.trim_end_matches(['\r', '\0']);
        // [offset:...] 可出现在任意位置（惯例在头部）；只取一次
        if let Some(rest) = line.strip_prefix("[offset:") {
            if let Some(end) = rest.find(']') {
                if let Ok(v) = rest[..end].trim().parse::<i64>() {
                    offset_ms = v;
                    continue;
                }
            }
        }

        // 提取行首连续时间戳：[mm:ss] [mm:ss.xx] [mm:ss.xxx]
        let mut stamps = Vec::new();
        let mut rest = line.trim_start();
        while rest.starts_with('[') {
            match extract_stamp(rest) {
                Some((ms, after)) => {
                    stamps.push(ms);
                    rest = after;
                }
                None => break,
            }
        }

        let content = rest.trim_end();
        if stamps.is_empty() {
            // 元数据标签 [ti:...] 等不进纯文本；纯文本歌词才有意义
            if !is_meta_tag(line) {
                plain.push(LyricLine { time_ms: 0, text: content.to_string() });
            }
        } else {
            for ms in stamps {
                timed.push(LyricLine { time_ms: ms, text: content.to_string() });
            }
        }
    }

    let offset_applied = offset_ms;
    for l in &mut timed {
        l.time_ms += offset_applied;
    }
    (timed, plain, offset_ms)
}

/// 解析行首 `[mm:ss(.xx|xxx)]`，返回 (毫秒, 剩余字符串)。
/// 不匹配（如 `[ti:xx]`、`[00]`）返回 None。
fn extract_stamp(s: &str) -> Option<(i64, &str)> {
    let end = s.find(']')?;
    let inner = &s[1..end];
    let (min, rest) = inner.split_once(':')?;
    let minutes = min.trim().parse::<i64>().ok()?;
    // 分钟可能带负号？LRC 标准不允许；解析失败就放弃
    if minutes < 0 {
        return None;
    }
    let (secs, millis) = match rest.split_once('.') {
        Some((sec, frac)) => {
            let milliseconds = if frac.len() >= 3 {
                frac[..3].parse::<i64>().ok()?
            } else {
                // 1–2 位小数按百分秒/十分秒解释
                let f = frac.parse::<i64>().ok()?;
                f * 10_i64.pow(3 - frac.len() as u32)
            };
            (sec.trim().parse::<i64>().ok()?, milliseconds)
        }
        None => (rest.trim().parse::<i64>().ok()?, 0),
    };
    if !(0..60).contains(&secs) {
        return None;
    }
    let after = &s[end + 1..];
    Some((minutes * 60_000 + secs * 1_000 + millis, after))
}

/// `[ti:...]` `[ar:...]` 等元数据标签行（不能与时间戳混淆：它们冒号后非数字）。
fn is_meta_tag(line: &str) -> bool {
    let inner = match line.trim_start().strip_prefix('[') {
        Some(rest) => match rest.find(']') {
            Some(end) => &rest[..end],
            None => return false,
        },
        None => return false,
    };
    matches!(inner.split(':').next(), Some("ti" | "ar" | "al" | "by" | "length" | "re" | "ve" | "tool"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_lrc_with_offset_and_multi_stamps() {
        let src = "[ti:x]\n[offset:500]\n[00:01.00][00:05.5]第一行\n[00:03]第二行\n";
        let l = parse_lrc(src.as_bytes());
        assert!(l.synced);
        assert_eq!(l.offset_ms, 500);
        assert_eq!(l.encoding, "UTF-8");
        let times: Vec<i64> = l.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, vec![1500, 3500, 6000]); // offset 已折入且升序
        assert_eq!(l.lines[0].text, "第一行");
        assert_eq!(l.lines[1].text, "第二行");
        assert_eq!(l.lines[2].text, "第一行");
    }

    #[test]
    fn plain_text_falls_back_to_unsynced() {
        let src = "第一段\r\n\r\n第二段\r\n";
        let l = parse_lrc(src.as_bytes());
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[1].text, "第二段");
    }

    #[test]
    fn gbk_is_detected_when_invalid_utf8() {
        // “测试” 的 GBK：B2 E2 CA D4
        let bytes = [0xB2, 0xE2, 0xCA, 0xD4];
        let l = parse_lrc(&bytes);
        assert_eq!(l.encoding, "GBK");
        assert_eq!(l.lines[0].text, "测试");
    }

    #[test]
    fn utf16le_bom_is_decoded() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "你好".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let l = parse_lrc(&bytes);
        assert_eq!(l.encoding, "UTF-16LE");
        assert_eq!(l.lines[0].text, "你好");
    }

    #[test]
    fn meta_tags_and_garbage_brackets_are_ignored() {
        let src = "[ar:某人]\n[length:3:21]\n[00:01.00]真实行\n[这不是时间]也不是\n";
        let l = parse_lrc(src.as_bytes());
        assert!(l.synced);
        // "[这不是时间]也不是" 无有效时间戳 → 进纯文本，但 synced 时被丢弃
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.lines[0].text, "真实行");
    }

    #[test]
    fn same_dir_lrc_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("song one.flac");
        std::fs::write(&audio, b"x").unwrap();
        std::fs::write(dir.path().join("song one.lrc"), "[00:01.00]hi\n").unwrap();
        let l = read_lyrics(&audio).unwrap();
        assert!(l.synced);
        assert_eq!(l.lines[0].time_ms, 1000);
    }

    #[test]
    fn missing_lrc_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("song.flac");
        std::fs::write(&audio, b"x").unwrap();
        assert!(read_lyrics(&audio).is_none());
    }
}
