//! 拖放 / 文件选择的路径分类（docs/52 §5）。
//!
//! **分类器只有这一处**：UI 只负责把 `paths` 递过来并展示结果，后端（拖放、
//! 文件对话框、命令行）共用同一套判定。理由是本仓已经吃过一次"两处各写一份
//! 规则"的亏——`is_http_url` 在引擎里大小写不敏感、在 Actor 里只认小写，
//! `HTTP://` 于是被当流打开却标成 File（进度条可拖，一拖报错）。
//!
//! 判定口径：
//! - 音频扩展名表取 `aiya_audio::decoder::SUPPORTED_AUDIO_EXT`（唯一源）；
//! - "认识但解不了"的（Opus/WMA/APE/WavPack/DSD）报格式名而不是"文件坏了"；
//! - `.m3u8` = HLS 清单；`.m3u` **嗅内容**（含 `#EXT-X-` 就是 HLS，否则是
//!   老式歌单）——扩展名相同而语义不同，这一点跟 TS 段用内容嗅探是同一个判断；
//! - 目录**不识别**（用户 2026-10-31 拍板）：一次误拖不该触发几万次 stat 的
//!   递归扫描，那是曲库导入的活。

use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

/// 一个拖入条目的分类结果。wire 上是 `kind` 字符串 + 人话 `note`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DroppedKind {
    /// 能直接播的音频文件
    Audio,
    /// HLS 清单（.m3u8 / 含 #EXT-X- 的 .m3u）
    HlsPlaylist,
    /// 老式 m3u 歌单（要展开成条目）
    Playlist,
    /// 文件夹：v1 不识别
    Directory,
    /// 不认识 / 解不了
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DroppedItem {
    pub path: String,
    pub kind: DroppedKind,
    /// 人话补充：不支持时说格式名，文件夹说"文件夹"
    pub note: String,
    /// 展示名（音频取文件名，播放列表取清单名）
    pub label: String,
}

/// 判定一个 `.m3u` 是 HLS 清单还是老式歌单：读开头一段找 `#EXT-X-`。
///
/// 只读 8KB：HLS 的标签集中在清单开头（`#EXT-X-TARGETDURATION` 必在前几行），
/// 老式歌单则是 `#EXTM3U` + 一堆 `#EXTINF`，两者一眼可分。
fn looks_like_hls(path: &Path) -> bool {
    use std::io::Read;
    const SNIFF: u64 = 8192;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = Vec::new();
    if file.take(SNIFF).read_to_end(&mut buf).is_err() {
        return false;
    }
    String::from_utf8_lossy(&buf).contains("#EXT-X-")
}

/// 单个路径的分类。纯函数（除了 `.m3u` 的内容嗅探要读盘）。
pub fn classify(path: &Path) -> DroppedItem {
    let path_str = path.to_string_lossy().into_owned();
    let label = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.clone());

    if path.is_dir() {
        return DroppedItem {
            path: path_str,
            kind: DroppedKind::Directory,
            note: "文件夹".into(),
            label,
        };
    }
    if !path.exists() {
        return DroppedItem {
            path: path_str,
            kind: DroppedKind::Unsupported,
            note: "文件不存在".into(),
            label,
        };
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "m3u8" => DroppedItem {
            path: path_str,
            kind: DroppedKind::HlsPlaylist,
            note: "HLS 清单".into(),
            label,
        },
        "m3u" if looks_like_hls(path) => DroppedItem {
            path: path_str,
            kind: DroppedKind::HlsPlaylist,
            note: "HLS 清单".into(),
            label,
        },
        "m3u" => DroppedItem {
            path: path_str,
            kind: DroppedKind::Playlist,
            note: "m3u 歌单".into(),
            label,
        },
        _ if aiya_audio::decoder::SUPPORTED_AUDIO_EXT.contains(&ext.as_str()) => DroppedItem {
            path: path_str,
            kind: DroppedKind::Audio,
            note: String::new(),
            label,
        },
        _ => {
            // "认识但解不了"报格式名（用户该换文件/转码，不是重试）
            let note = aiya_audio::decoder::unsupported_ext_name(path)
                .map(|name| format!("{name} 不支持"))
                .unwrap_or_else(|| "不支持的格式".into());
            DroppedItem {
                path: path_str,
                kind: DroppedKind::Unsupported,
                note,
                label,
            }
        }
    }
}

/// 展开老式 m3u：相对路径按清单所在目录解析，`#` 开头的行全部忽略。
///
/// 只做"列条目"这一件事：不查库、不去重、不排序。真正的分类仍由
/// [`classify`] 逐条做——歌单里混进一个 wma 不该让整张单报废，它只是被跳过。
pub fn expand_playlist(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read(path).map_err(|e| format!("读歌单失败 {}：{e}", path.display()))?;
    let text = String::from_utf8_lossy(&text);
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 网络地址原样保留（歌单里可以有远程条目）
        if line.contains("://") {
            out.push(line.to_string());
            continue;
        }
        let p = PathBuf::from(line);
        let resolved = if p.is_absolute() { p } else { base.join(p) };
        out.push(resolved.to_string_lossy().into_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn audio_extensions_are_recognised() {
        let dir = tmp();
        for name in ["a.flac", "b.MP3", "c.m4a", "d.wav", "e.ogg", "f.aiff"] {
            let p = dir.path().join(name);
            std::fs::write(&p, b"x").unwrap();
            assert_eq!(classify(&p).kind, DroppedKind::Audio, "{name}");
        }
    }

    /// 认识但解不了的格式要报格式名（不是"文件坏了"）
    #[test]
    fn known_unsupported_reports_format_name() {
        let dir = tmp();
        for (name, expect) in [("a.wma", "WMA"), ("b.opus", "Opus"), ("c.dsf", "DSD")] {
            let p = dir.path().join(name);
            std::fs::write(&p, b"x").unwrap();
            let item = classify(&p);
            assert_eq!(item.kind, DroppedKind::Unsupported, "{name}");
            assert!(item.note.contains(expect), "{name}: {}", item.note);
        }
    }

    /// .m3u 按内容分流：有 #EXT-X- 的是 HLS 清单，否则是老式歌单
    #[test]
    fn m3u_is_dispatched_by_content() {
        let dir = tmp();
        let hls = dir.path().join("live.m3u");
        std::fs::write(
            &hls,
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg0.ts\n",
        )
        .unwrap();
        assert_eq!(classify(&hls).kind, DroppedKind::HlsPlaylist);

        let plain = dir.path().join("songs.m3u");
        std::fs::write(&plain, "#EXTM3U\n#EXTINF:180,Artist - Title\nD:/m/a.flac\n").unwrap();
        assert_eq!(classify(&plain).kind, DroppedKind::Playlist);
    }

    /// 目录不识别（用户拍板：一次误拖不该触发递归扫描）
    #[test]
    fn directory_is_not_expanded() {
        let dir = tmp();
        let item = classify(dir.path());
        assert_eq!(item.kind, DroppedKind::Directory);
        assert_eq!(item.note, "文件夹");
    }

    #[test]
    fn missing_file_is_reported_as_missing() {
        let dir = tmp();
        let item = classify(&dir.path().join("nope.flac"));
        assert_eq!(item.kind, DroppedKind::Unsupported);
        assert!(item.note.contains("不存在"));
    }

    /// 展开 m3u：相对路径按清单目录解析，注释与空行忽略，URL 原样保留
    #[test]
    fn playlist_entries_resolve_relative_to_the_file() {
        let dir = tmp();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let m3u = dir.path().join("list.m3u");
        std::fs::write(
            &m3u,
            "\u{feff}#EXTM3U\n#EXTINF:1,x\nsub/a.flac\n\nhttps://hls.test/live.m3u8\n# 注释\n",
        )
        .unwrap();
        let entries = expand_playlist(&m3u).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].ends_with("sub/a.flac") || entries[0].ends_with("sub\\a.flac"));
        assert!(entries[0].starts_with(dir.path().to_string_lossy().as_ref()));
        assert_eq!(entries[1], "https://hls.test/live.m3u8");
    }
}
