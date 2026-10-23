//! 标题/名称规范化：搜索与排序键共用的 `*_norm`。
//!
//! 策略（docs/02 §5.3）：小写 + 折叠空白 + 去首尾。不上 NFKD 音译 ——
//! 中文场景收益为零，还会把「ａｂｃ」之外的全角数字变掉引起困惑。

pub fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::norm;

    #[test]
    fn basic() {
        assert_eq!(norm("  Foo   BAR "), "foo bar");
        assert_eq!(norm("周杰伦"), "周杰伦");
        assert_eq!(norm("ＭＩＤＮＩＧＨＴ"), "ｍｉｄｎｉｇｈｔ"); // 全角小写化即可，不做音译
    }
}
