// enrichment 域的 server 侧小件：来源名归一化（与桌面壳 enrichment.rs 同口径）。

pub const DEFAULT_SOURCE: &str = aiya_beets_store::SOURCE_XTRACTOR;

pub fn normalize_source(raw: Option<&str>) -> Result<String, String> {
    let s = raw.unwrap_or_default().trim();
    if s.is_empty() {
        return Ok(DEFAULT_SOURCE.to_string());
    }
    if s.len() > 64
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!("invalid enrichment source name: {s}"));
    }
    Ok(s.to_string())
}
