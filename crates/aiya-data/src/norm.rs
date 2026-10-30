//! 标题/名称规范化：搜索与排序键共用的 `*_norm`。
//!
//! 策略（docs/02 §5.3）：小写 + 折叠空白 + 去首尾。不上 NFKD 音译 ——
//! 中文场景收益为零，还会把「ａｂｃ」之外的全角数字变掉引起困惑。

pub fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// LIKE 通配符按字面转义（配合 `ESCAPE '\'`）：`%`、`_`、`\` 三个字符前加反斜杠。
///
/// 本 crate 内所有"把用户输入拼进 LIKE 模式"的查询（repo 搜索、facets 浏览、
/// smart 规则 contains、scanner 的路径前缀对账）都必须先过这里——漏转义的
/// 表现是"搜 `50%` 匹配出全部歌曲"这类静默错误结果。输入是否先 norm 由
/// 调用方决定（本函数只做转义，不改字符）。
pub(crate) fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{escape_like, norm};

    #[test]
    fn basic() {
        assert_eq!(norm("  Foo   BAR "), "foo bar");
        assert_eq!(norm("周杰伦"), "周杰伦");
        assert_eq!(norm("ＭＩＤＮＩＧＨＴ"), "ｍｉｄｎｉｇｈｔ"); // 全角小写化即可，不做音译
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("50%_off\\x"), "50\\%\\_off\\\\x");
        assert_eq!(escape_like("普通文本"), "普通文本", "普通字符原样保留");
    }
}
