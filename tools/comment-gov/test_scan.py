#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""scan.py 状态机的边界用例自测。全部断言通过则静默退出 0。

每条用例是一段合成源码 + 期望分类序列（c=code, m=comment, k=mixed, b=blank）。
这些用例锁住的是"危险方向"：字符串内容不得判成注释（会被误删），
注释不得判成代码（会漏删并在剥注释比对时掩盖真实差异）。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import scan as S

CASES = [
    # ---------------- rust ----------------
    ("rust", '''let s = "http://x"; // tail
// real''', ['k', 'm']),
    ("rust", '''let r = r#"a "q" {{b}}"#;
// real''', ['c', 'm']),
    ("rust", '''/* a /* b */ still */
// real''', ['m', 'm']),
    ("rust", '''let c = '*';
let l: &'static str = "x";
// real''', ['c', 'c', 'm']),
    ("rust", '''let b = b"bytes";
let s = "multi
// not a comment
tail";
// real''', ['c', 'c', 'c', 'c', 'm']),
    ("rust", '''//! module doc
/// item doc with 'quote and "quote
fn f() {}
// real''', ['m', 'm', 'c', 'm']),
    ("rust", '''writeln!(w, r#"{{"id":1}}"#);
// real''', ['c', 'm']),
    # ---------------- js / ts ----------------
    ("js", '''const u = "http://x"; // tail
// real''', ['k', 'm']),
    ("js", '''const glsl = `
/* not a comment */
precision highp;
`;
// real''', ['c', 'c', 'c', 'c', 'm']),
    ("js", '''const s = `${ {a: 1}.a }`;
// real''', ['c', 'm']),
    ("js", '''const re = /a\\/b[/]c/g;
const d = a / b / c; // tail
// real''', ['c', 'k', 'm']),
    ("js", '''/* one */ /* two */
// real''', ['m', 'm']),
    ("js", '''const t = `a ${`b ${x}`} c`;
// real''', ['c', 'm']),
    # ---------------- svelte ----------------
    ("svelte", '''<div
  onscroll={() => {
    // expr comment
    handle();
  }}
>
  {#if x}
    <p>{'{'}</p>
  {/if}
</div>
<!-- html note -->''', ['c', 'c', 'm', 'c', 'c', 'c', 'c', 'c', 'c', 'c', 'm']),
    ("svelte", '''<script lang="ts">
const u = "http://x";
// real
</script>
<!-- markup note -->''', ['c', 'c', 'm', 'c', 'm']),
    ("svelte", '''<style>
/* css note */
.a { color: red; }
</style>''', ['c', 'm', 'c', 'c']),
    ("css", '''/* header */
.a { content: "/* not */"; }
/* real */''', ['m', 'c', 'm']),
    # ---------------- python ----------------
    ("python", '''x = "# not comment"  # tail
# real''', ['k', 'm']),
    ("python", '''"""Module doc.
# not a comment
More text.
"""
# real''', ['c', 'c', 'c', 'c', 'm']),
    ("python", '''spec = f"{x:#04x}"  # tail
# real''', ['k', 'm']),
    ("python", '''p = r"\\d+ # x"
# real''', ['c', 'm']),
    ("python", '''#!/usr/bin/env python3
# header line
# real''', ['c', 'm', 'm']),
    ("python", '''s = 'a\\'b'
# real''', ['c', 'm']),
]


def run():
    fails = 0
    for lang, src, expect in CASES:
        lines = src.split('\n')
        kinds, _, _, _ = S.analyze(lines, lang)
        got = [{'blank': 'b', 'comment': 'm', 'code': 'c', 'mixed': 'k'}[k] for k in kinds]
        if got != expect:
            fails += 1
            print(f"FAIL [{lang}]")
            for i, (ln, g, e) in enumerate(zip(lines, got, expect)):
                mark = ' ' if g == e else '<<<'
                print(f"  L{i+1} got={g} want={e} {mark} {ln[:60]!r}")
        else:
            print(f"ok   [{lang}] {len(expect)} 行")
    if fails:
        print(f"\n{fails} 条用例失败")
        sys.exit(1)
    print("\n全部通过")


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    run()
