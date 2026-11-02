#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""终验：对 git 工作区全部已修改源码文件，剥注释+丢空行后与 HEAD 逐行比对。

这是 docs/60 §4 "剥离注释后逐字节一致" 的机器判据，也是
.workbuddy 2026-11-01 教训 2 的正确口径（教训 2 的错误版本是
"行数不等就跳过不比内容"）。剥注释由 scan.analyze 的状态机完成，
字符串/模板/原始字符串里的内容一律保留。

另外校验：BOM 与换行风格逐文件保持不变。
退出码 0 = 全部一致；1 = 有差异（列出前 20 处）。
"""

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
import scan as S

ROOT = S.ROOT


def git(args):
    return subprocess.run(['git', '-C', str(ROOT)] + args, capture_output=True).stdout


def code_lines(lines, lang):
    kinds, _, _, _ = S.analyze(lines, lang)
    return [ln for ln, k in zip(lines, kinds) if k in ('code', 'mixed')]


def main():
    status = git(['status', '--porcelain']).decode('utf-8', errors='replace')
    checked = failed = 0
    fails = []
    for ln in status.splitlines():
        if not ln.startswith(' M ') and not ln.startswith('M '):
            continue
        rel = ln[3:].strip().strip('"').replace('\\', '/')
        p = ROOT / rel
        if p.suffix not in S.LANG_BY_EXT:
            print(f'-- 跳过（非源码）：{rel}')
            continue
        head = git(['show', f'HEAD:{rel}'])
        work = p.read_bytes()
        head_bom = head.startswith(b'\xef\xbb\xbf')
        work_bom = work.startswith(b'\xef\xbb\xbf')
        head_crlf = b'\r\n' in head
        work_crlf = b'\r\n' in work
        hl, he, hb, ht = S.load_text_bytes(head)
        wl, we, wb, wt = S.load_text_bytes(work)
        checked += 1
        problems = []
        if head_bom != work_bom:
            problems.append('BOM 不一致')
        if (he or '\n') != (we or '\n'):
            problems.append('换行风格不一致')
        cl_head = code_lines(hl, S.LANG_BY_EXT[p.suffix])
        cl_work = code_lines(wl, S.LANG_BY_EXT[p.suffix])
        if cl_head != cl_work:
            n = sum(1 for a, b in zip(cl_head, cl_work) if a != b)
            problems.append(f'剥注释后代码行不一致（{len(cl_head)} vs {len(cl_work)} 行）')
            idx = next(i for i, (a, b) in enumerate(zip(cl_head, cl_work)) if a != b) \
                if len(cl_head) == len(cl_work) else 0
            problems.append(f'  首个差异@{idx}: HEAD={cl_head[idx][:60]!r} 工作区={cl_work[idx][:60]!r}')
        if problems:
            failed += 1
            fails.append(rel)
            print(f'!! {rel}')
            for x in problems:
                print(f'   {x}')
        else:
            print(f'ok  {rel}  （代码行 {len(cl_head)}）')
    print(f'\n共 {checked} 个源码文件，{failed} 个不一致')
    if fails:
        sys.exit(1)


if __name__ == '__main__':
    main()
