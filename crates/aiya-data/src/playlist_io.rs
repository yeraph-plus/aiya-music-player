//! m3u / m3u8 播放列表的解析与渲染（纯函数：无 IO、无 SQL、无 Tauri）。
//!
//! 为什么纯函数：解析规则琐碎（BOM、URL 解码、相对路径、EXTINF 容错、
//! HLS 拒绝），只有无 IO 的纯函数才能用全矩阵单测钉死——文件层就剩
//! "读进来、传进来、写出去"三行胶水，错误全靠这里的类型表达。
//!
//! 格式口径（docs/22 G2）：
//! - m3u8 强制 UTF-8（容忍 BOM）；裸 m3u 也只吃 UTF-8，**不做 GBK 探测**（D5），
//!   解码失败由调用方报 encodingUnsupported
//! - HLS 流媒体 manifest（出现任何 `#EXT-X-` 标签）显式拒绝——它不是静态
//!   播放列表，静默解析出一个只含一个 URL 的"歌单"更害人
//! - `#EXTINF:<秒>,<artist> - <title>`，分隔容错：没有 " - " 就整段是标题
//! - 相对路径按 m3u 所在目录展开后**做词法规范化**（`.`/`..`）：不规范化的话
//!   `../a.flac` 与库里落账的绝对路径字符串不等，精确对账会把整份歌单报成未匹配

use std::path::{Path, PathBuf};

/// 解析出的一个条目。`target` 已做 URL 解码与相对路径解析（锚定 `base_dir`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uEntry {
    /// 文件中的行号（1 基）。未匹配报告要能指回原文件
    pub line: usize,
    /// 原始文本（URL 解码前），报告里原样展示
    pub raw: String,
    pub target: PathBuf,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub duration_sec: Option<i64>,
}

/// 解析失败。变体即错误分类——调用方按变体映射 `ApiErrorCode`，不解析文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseFailure {
    /// HLS 流媒体 manifest（出现 `#EXT-X-` 标签），携带首个命中的标签名
    HlsManifest { tag: String },
    /// 没有任何条目（空文件 / 全是注释）
    Empty,
}

/// 单行长度上限。防御性：m3u 没有行长度语义，超长行只可能是坏文件，
/// 直接跳过而不是让内存跟着炸。
const MAX_LINE_CHARS: usize = 8192;

/// 解析播放列表文本。`base_dir` 是 m3u 文件所在目录（相对路径的锚）。
pub fn parse_playlist(text: &str, base_dir: &Path) -> Result<Vec<M3uEntry>, ParseFailure> {
    // m3u8/m3u 统一容忍 UTF-8 BOM（Windows 记事本保存 UTF-8 会带）
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let mut entries = Vec::new();
    // EXTINF 元数据挂在"下一个非注释条目行"上：先记着，遇到条目行消费掉
    let mut pending_extinf: Option<(Option<i64>, Option<String>, Option<String>)> = None;

    for (idx, line) in text.lines().enumerate() {
        let line = line.trim_end_matches(['\r']);
        let line_no = idx + 1;
        if line.chars().count() > MAX_LINE_CHARS {
            continue; // 坏行跳过（测试锚点：不 panic、不 OOM）
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("#EXTM3U") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#EXTINF:") {
            pending_extinf = Some(parse_extinf(rest));
            continue;
        }
        if let Some(tag) = trimmed.strip_prefix("#EXT-X-") {
            // 到 tag 名为止（#EXT-X-TARGETDURATION:10 → "TARGETDURATION"）
            let name: String = tag.chars().take_while(|c| *c != ':').collect();
            return Err(ParseFailure::HlsManifest { tag: name });
        }
        if trimmed.starts_with('#') {
            continue; // 其他注释（#EXTGR3、#PLAYLIST、#EXTALB…）一律忽略
        }
        let (duration_sec, artist, title) = pending_extinf.take().unwrap_or((None, None, None));
        entries.push(M3uEntry {
            line: line_no,
            raw: trimmed.to_string(),
            target: entry_target(trimmed, base_dir),
            artist,
            title,
            duration_sec,
        });
    }

    if entries.is_empty() {
        // 全是注释/空行也是 Empty——"导入成功但 0 首"比报错更难排查
        return Err(ParseFailure::Empty);
    }
    Ok(entries)
}

/// `#EXTINF:` 之后的文本 → (duration, artist, title)。
/// 宽进：时长解析失败不报错（这行还想活），标题没有 " - " 分隔就整段是标题。
fn parse_extinf(rest: &str) -> (Option<i64>, Option<String>, Option<String>) {
    let (dur_part, title_part) = match rest.split_once(',') {
        Some((d, t)) => (Some(d), t.trim()),
        None => (None, ""),
    };
    let duration_sec = dur_part.and_then(|d| d.trim().parse::<f64>().ok()).map(|f| f as i64);
    // "Artist - Title" 约定：按第一个 " - " 切；不是这个形状就整段是标题
    let (artist, title) = match title_part.split_once(" - ") {
        Some((a, t)) => (Some(a.trim().to_string()), Some(t.trim().to_string())),
        None => (None, if title_part.is_empty() { None } else { Some(title_part.to_string()) }),
    };
    (duration_sec, artist, title)
}

/// 条目行 → 目标路径：URL 解码 + 相对路径锚定 base_dir + **词法规范化**。
///
/// 规范化不是洁癖，是正确性要求：`D:/m/playlists/../a.flac` 与扫描器落库的
/// `D:/m/a.flac` 是同一个文件，但字符串不等 → 精确对账（path_lower）会把
/// 整份"歌单放在子目录、条目写 `../`"的 m3u 全部报成未匹配。
///
/// 为什么用词法而不是 `fs::canonicalize`：① canonicalize 在 Windows 上返回
/// `\\?\D:\...` 前缀，还得再剥一次；② 它要求文件存在，缺一个文件就把整条
/// 路径变成不可解析；③ 我们只关心 `.`/`..`/分隔符，符号链接交给扫描器。
fn entry_target(raw: &str, base_dir: &Path) -> PathBuf {
    let decoded = percent_decode(raw.trim());
    if let Some(rest) = decoded.strip_prefix("file://") {
        // file:///C:/m/x.flac → /C:/m/x.flac（Windows 的驱动器斜杠要去掉）
        let p = if cfg!(windows) && rest.len() >= 3 && rest.as_bytes()[0] == b'/' && rest.as_bytes()[2] == b':' {
            &rest[1..]
        } else {
            rest
        };
        return normalize_lexical(Path::new(p));
    }
    let p = PathBuf::from(&decoded);
    normalize_lexical(&if p.is_absolute() { p } else { base_dir.join(p) })
}

/// 纯词法规范化：吃掉 `.`、消掉 `..`（前面有可消的普通段时），不动文件系统。
/// 相对路径开头多余的 `..` 原样保留——那是真的上不去了。
fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut parts: Vec<Component<'_>> = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                // 只吃普通段：`C:\..` 或 `/..` 或开头的 `..` 都保留（已到根）
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                _ => parts.push(c),
            },
            other => parts.push(other),
        }
    }
    parts.iter().map(|c| c.as_os_str()).collect()
}

/// 最小 %XX 解码器：非法序列（% 后不是两个十六进制位）原样保留。
/// 刻意不引 url crate——只此一处用，不值得一个依赖。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).and_then(|h| {
                std::str::from_utf8(h).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
            });
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/* ---------------- 导出渲染 ---------------- */

/// 渲染输入：歌单内一首曲目的导出所需字段（DB JOIN 出来原样填）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportItem {
    pub path: String,
    pub artist: Option<String>,
    pub title: String,
    pub duration_ms: i64,
}

/// 渲染为 m3u/m3u8 文本。两种格式内容相同（都是 UTF-8，D5：不做 GBK），
/// 差异只在扩展名约定，所以渲染器不区分。
///
/// `relative = true` 时，位于 `base_dir`（导出文件的目录）之下的路径写成
/// 相对路径（`/` 分隔，跨平台可交换）；之外的路径保持绝对——相对化不了
/// 就别硬凑，坏路径比长路径害人。
pub fn render_playlist(items: &[ExportItem], base_dir: &Path, relative: bool) -> String {
    let mut out = String::from("#EXTM3U\n");
    for item in items {
        let sec = (item.duration_ms / 1000).max(0);
        let label = match &item.artist {
            Some(a) if !a.trim().is_empty() => format!("{a} - {}", item.title),
            _ => item.title.clone(),
        };
        out.push_str(&format!("#EXTINF:{sec},{label}\n"));
        let path = if relative { relative_to(&item.path, base_dir) } else { item.path.clone() };
        out.push_str(&path);
        out.push('\n');
    }
    out
}

/// 尽力相对化：`path` 在 `base_dir` 之下（大小写不敏感，Windows 语义）
/// 才相对化并把分隔符统一成 `/`；否则原样返回绝对路径。
fn relative_to(path: &str, base_dir: &Path) -> String {
    let base = base_dir.to_string_lossy();
    let base = base.trim_end_matches(['\\', '/']);
    let base_lower = base.to_lowercase();
    let path_norm = path.replace('/', "\\");
    let path_lower = path_norm.to_lowercase();
    let sep = format!("{base_lower}\\");
    if let Some(rest) = path_lower.strip_prefix(&sep) {
        // 按长度切原串（大小写不同不能用切片），分隔符统一 /
        let skip = base.len() + 1;
        if let Some(orig) = path_norm.get(skip..) {
            return orig.replace('\\', "/");
        }
        return rest.to_string();
    }
    if path_lower == base_lower {
        return String::new();
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<M3uEntry> {
        parse_playlist(text, Path::new("/m")).unwrap()
    }

    /// 期望值也走一遍同样的规范化：components 重组会把 `/` 变成 `\`，
    /// 直接写字面量比的是分隔符风格，不是语义。
    fn p(s: &str) -> PathBuf {
        normalize_lexical(Path::new(s))
    }

    #[test]
    fn basic_extinf_artist_title() {
        let es = parse("#EXTM3U\n#EXTINF:180,周杰伦 - 晴天\nsongs/qing.mp3\n\n# 注释\nbare.flac\n");
        assert_eq!(es.len(), 2);
        assert_eq!(es[0].line, 3);
        assert_eq!(es[0].duration_sec, Some(180));
        assert_eq!(es[0].artist.as_deref(), Some("周杰伦"));
        assert_eq!(es[0].title.as_deref(), Some("晴天"));
        assert_eq!(es[0].target, p("/m/songs/qing.mp3"), "相对路径锚定 base_dir");
        assert_eq!(es[1].title, None, "裸路径没有 EXTINF 就没有标题");
        assert_eq!(es[1].target, p("/m/bare.flac"));
    }

    #[test]
    fn extinf_tolerances() {
        // 浮点秒 / 裸标题 / 没有时长 / 空标题段
        let es = parse("#EXTINF:12.9,a\nf1\n#EXTINF:,裸标题\nf2\n#EXTINF:30,\nf3\n");
        assert_eq!(es[0].duration_sec, Some(12));
        assert_eq!(es[1].artist, None);
        assert_eq!(es[1].title.as_deref(), Some("裸标题"));
        assert_eq!(es[2].duration_sec, Some(30));
        assert_eq!(es[2].title, None, "逗号后为空 = 没有标题");
    }

    #[test]
    fn url_and_percent_decoding() {
        let es = parse("file:///C:/My%20Music/a.flac\nC:/My%20Music/b.flac\n");
        assert_eq!(es[0].target, p("C:/My Music/a.flac"));
        assert_eq!(es[1].target, p("C:/My Music/b.flac"));
    }

    /// 相对路径必须规范化：`..` 不消掉的话，导入侧按 path_lower 精确对账
    /// 永远匹配不上扫描器落库的绝对路径（这是"歌单放子目录"的常规形态）。
    #[test]
    fn parent_dir_is_normalized_against_playlist_dir() {
        let base = Path::new("D:/m/playlists");
        let es = parse_playlist(
            "#EXTM3U\n../a.flac\n./b.flac\n../../c.flac\nsub/../d.flac\n",
            base,
        )
        .unwrap();
        assert_eq!(es[0].target, p("D:/m/a.flac"), ".. 要消掉");
        assert_eq!(es[1].target, p("D:/m/playlists/b.flac"), ". 要吃掉");
        assert_eq!(es[2].target, p("D:/c.flac"), "连级 .. 两个层级都要消");
        assert_eq!(es[3].target, p("D:/m/playlists/d.flac"));
    }

    /// 规范化不越界：到根之后 `..` 保留原样，不许静默上移成别处。
    #[test]
    fn normalization_does_not_climb_past_root() {
        // 相对 base 上不去的 .. 保留（消掉就等于凭空吞了真实层级）
        assert_eq!(
            parse_playlist("../../x.flac\n", Path::new("m")).unwrap()[0].target,
            Path::new("..").join("x.flac")
        );
        let t = parse_playlist("D:/../x.flac\n", Path::new("m")).unwrap()[0].target.clone();
        assert!(t.is_absolute(), "盘符根不可被 .. 吃掉：{t:?}");
        assert!(
            t.to_string_lossy().contains(".."),
            "根之后的 .. 应保留而不是静默上移：{t:?}"
        );
    }

    #[test]
    fn bom_and_crlf_tolerated() {
        let es = parse("\u{feff}#EXTM3U\r\n#EXTINF:1,x - y\r\na.mp3\r\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].title.as_deref(), Some("y"));
    }

    #[test]
    fn hls_manifest_is_rejected_with_tag_name() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:10,\nseg0.ts\n";
        match parse_playlist(text, Path::new("/m")) {
            Err(ParseFailure::HlsManifest { tag }) => assert_eq!(tag, "TARGETDURATION"),
            other => panic!("应拒绝 HLS manifest，实际 {other:?}"),
        }
    }

    #[test]
    fn empty_or_comment_only_is_empty_error() {
        assert_eq!(parse_playlist("", Path::new("/m")), Err(ParseFailure::Empty));
        assert_eq!(
            parse_playlist("#EXTM3U\n# 全是注释\n\n", Path::new("/m")),
            Err(ParseFailure::Empty)
        );
    }

    #[test]
    fn overlong_line_is_skipped_not_fatal() {
        let mut text = String::from("#EXTM3U\n");
        text.push_str(&"x".repeat(10_000));
        text.push_str("\nok.mp3\n");
        let es = parse(&text);
        assert_eq!(es.len(), 1, "超长行跳过，不影响其余条目");
        assert_eq!(es[0].raw, "ok.mp3");
    }

    /* ---------------- 渲染 ---------------- */

    fn item(path: &str, artist: Option<&str>, title: &str, ms: i64) -> ExportItem {
        ExportItem { path: path.into(), artist: artist.map(Into::into), title: title.into(), duration_ms: ms }
    }

    #[test]
    fn render_absolute_with_extinf() {
        let s = render_playlist(
            &[item("D:/m/a.flac", Some("A"), "T", 185_400), item("D:/m/b.mp3", None, "裸", 0)],
            Path::new("D:/out"),
            false,
        );
        assert_eq!(s, "#EXTM3U\n#EXTINF:185,A - T\nD:/m/a.flac\n#EXTINF:0,裸\nD:/m/b.mp3\n");
    }

    #[test]
    fn render_relative_under_base_dir() {
        let s = render_playlist(
            &[
                item("D:/m/songs/a.flac", None, "内", 1000),
                item("D:/other/b.flac", None, "外", 1000),
            ],
            Path::new("D:/m"),
            true,
        );
        assert!(s.contains("songs/a.flac\n"), "库内相对化：{s}");
        assert!(s.contains("D:/other/b.flac\n"), "库外保持绝对：{s}");
    }

    #[test]
    fn render_parse_roundtrip() {
        let items = vec![
            item("D:/m/a.flac", Some("歌手甲"), "歌一", 200_000),
            item("D:/m/sub/b.mp3", None, "歌二", 95_500),
        ];
        let text = render_playlist(&items, Path::new("D:/m"), true);
        let es = parse_playlist(&text, Path::new("D:/m")).unwrap();
        assert_eq!(es.len(), 2);
        assert_eq!(es[0].duration_sec, Some(200));
        assert_eq!(es[0].artist.as_deref(), Some("歌手甲"));
        assert_eq!(es[0].title.as_deref(), Some("歌一"));
        assert_eq!(es[1].target, p("D:/m/sub/b.mp3"), "相对路径回读要落回原位");
    }
}
