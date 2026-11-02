#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""注释机械治理 · 应用器。

三重门（任一失败 → 整个 scope 本批不写盘，退出码 1）：
  门A diff 行级白名单：old/new 逐行 diff，被增删的每一行必须在其所属版本里
     分类为 blank/comment；出现任何 code/mixed 行、或 ^\\s*#\\[ 属性行 → 失败。
  门B 属性行计数：#[...] 行总数 old == new（试跑批 #[test] 假绿的教训）。
  门C 剥注释比对：两边各删掉全部注释行与空行后，剩余行序列逐字节一致
     （"剥注释 + 丢全部空行再逐行比"，.workbuddy 2026-11-01 教训 2 的正确口径）。

护栏（防块泄漏）：
  - 行注释（// /// //!）删改无护栏（行尾即界）。
  - 块注释（/* */、<!-- -->）只在"完整单行块"或"内部行（不含开闭符）"上
    删除/改写；开符行未闭合、闭符行带内容的一律跳过并计入报告。
  - 第二批重排要求块完整闭合，且块内不得出现第二个开符/闭符（Rust 嵌套块注释）。

用法：
  python -I tools/comment-gov/apply.py --scope rust|frontend --batch 1|2 [--dry]
"""

import argparse
import difflib
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.stderr.reconfigure(encoding='utf-8', errors='replace')

import scan as S

REPORTS = Path(__file__).resolve().parent / 'reports'
MAXCOL = 100
SEP_CLASS = r'=\-*~#·•─━—―+/\\|'
SANDWICH_RE = re.compile(rf'^[{SEP_CLASS}]{{2,}}\s*(.+?)\s*[{SEP_CLASS}]{{2,}}$')
CJK_LETTER = re.compile(r'[\u4e00-\u9fff]')
CJK_PUNCT = re.compile(r'[\u3000-\u303f，。：；、！？）》」』]')


# ---------------------------------------------------------------- 块注释护栏

def line_marker_kind(line_text, marker):
    """块注释行的位置形态：open/close/interior/single/line。"""
    s = line_text.strip()
    if marker in ('line', 'doc', 'inner', 'pound'):
        return 'line'
    if marker == 'html':
        has_open = '<!--' in s
        has_close = '-->' in s
        if has_open and has_close:
            return 'single'
        if has_open:
            return 'open'
        if has_close:
            return 'close'
        return 'interior'
    # /* */（rust/js/css）
    has_open = '/*' in s
    has_close = '*/' in s
    if has_open and has_close:
        return 'single'
    if has_open:
        return 'open'
    if has_close:
        return 'close'
    return 'interior'


def block_line_editable(line_text, marker):
    """该行可否整行删除或改写（不得让块泄漏）。"""
    k = line_marker_kind(line_text, marker)
    return k in ('line', 'single', 'interior')


# ---------------------------------------------------------------- 第一批变换

def transform_batch1(lines, kinds, contents, markers):
    """返回 (new_lines, stats)。日期戳删行、纯分隔线删行、三明治剥横线。"""
    n = len(lines)
    delete = [False] * n
    stats = {'date': 0, 'sep': 0, 'sandwich': 0, 'doc_skip': 0,
             'guarded_date': 0, 'guarded_sep': 0, 'guarded_sandwich': 0}
    for i in range(n):
        if kinds[i] != 'comment':
            continue
        # 操作性行：PEP 263 编码声明与 emacs/vim 模式行不做任何变换
        if re.match(r'^\s*#\s*-\*-', lines[i]) or (i < 2 and re.search(r'coding[:=]', contents[i])):
            continue
        c = contents[i].strip()
        mk = markers[i]
        editable = block_line_editable(lines[i], mk)
        # 日期戳：整行删
        if any(rx.search(contents[i]) for rx in S.DATE_PATTERNS.values()):
            if not editable:
                stats['guarded_date'] += 1
                continue
            delete[i] = True
            stats['date'] += 1
            continue
        # 纯分隔线：整行删（doc/inner 是 markdown 文档块，装饰变换会毁表格与
        # setext/标题结构，一律跳过，见 refresh.rs 教训）
        if S.is_separator_content(c):
            if mk in ('doc', 'inner'):
                stats['doc_skip'] += 1
                continue
            if not editable:
                stats['guarded_sep'] += 1
                continue
            delete[i] = True
            stats['sep'] += 1
            continue
        # 三明治：剥横线留标题（仅限非文档块）
        m = SANDWICH_RE.match(c)
        if m and mk not in ('doc', 'inner'):
            mid = m.group(1).strip()
            if mid and S.is_separator_content(mid) is False:
                new = rewrite_comment_line(lines[i], mk, mid, old_content=contents[i].strip())
                if new is not None:
                    lines[i] = new
                    contents[i] = mid
                    stats['sandwich'] += 1
                else:
                    stats['guarded_sandwich'] += 1
    if not any(delete):
        return lines, stats
    return collapse_blanks(lines, delete, kinds), stats


def rewrite_comment_line(line_text, marker, new_content, old_content=None):
    """按标记形态重写一行注释的内容。失败返回 None（护栏）。

    块注释（/* */、<!-- -->）走"原行内逐字替换"：开闭符原样保留，
    因此对开符行/闭符行也安全（只用于三明治剥横线，不用于删行）。
    """
    if marker in ('line', 'doc', 'inner', 'pound'):
        prefix = {'line': '// ', 'doc': '/// ', 'inner': '//! ', 'pound': '# '}[marker]
        return line_text[:len(line_text) - len(line_text.lstrip())] + prefix + new_content
    if old_content and old_content in line_text:
        return line_text.replace(old_content, new_content, 1)
    return None


def collapse_blanks(lines, delete, kinds):
    """删除标记行后，在删除接缝处收敛连续空行（只减不增，空行变动合法）。"""
    n = len(lines)
    out = []
    i = 0

    def is_blank(idx):
        return 0 <= idx < n and lines[idx].strip() == ''

    while i < n:
        if not delete[i]:
            out.append(lines[i])
            i += 1
            continue
        j = i
        while j < n and delete[j]:
            j += 1
        # 删除区间 [i, j)：接缝左侧是 out[-1]，右侧是 lines[j]
        if out and j < n and out[-1].strip() == '' and lines[j].strip() == '':
            out.pop()  # 两侧皆空 → 弃一个
        elif not out and j < n and lines[j].strip() == '':
            pass  # 文件头接缝：右侧空行保留（等价于原文件以注释开头）
        i = j
    return out


# ---------------------------------------------------------------- 第二批变换

def block_skip_reason(b, lines, kinds, contents, markers, lang, first_code):
    """第二批跳过判定；返回原因或 None（可压缩）。"""
    if b[-1] < first_code:
        return 'L1文件头'
    # L2 契约层：Rust /// 与 /*!、块内混有文档行的一律不压（连同其附注整块留给语义批）；
    # TS/Svelte 的 /** JSDoc 同为契约层（docs/60 §3）。
    if lang == 'rust' and any(markers[i] in ('doc', 'inner') for i in b):
        return 'L2文档注释'
    if lines[b[0]].lstrip().startswith(('/**', '/*!')):
        return 'L2文档注释'
    cbs = [contents[i] for i in b]
    if re.match(r'^(契约：|场景：)', cbs[0].lstrip()):
        return '契约场景标记'
    joined = '\n'.join(cbs)
    if '```' in joined:
        return '代码围栏'
    if S.TABLE_RE.search(joined):
        return '表格结构'
    if any(S.EVIDENCE_RE.search(c) for c in cbs):
        return '证据引用'
    if any(S.LIST_RE.match(c) for c in cbs):
        return '列表结构'
    # 块完整性护栏：整个块恰好一开一闭、闭符在末行 → 整块替换安全
    mk0 = markers[b[0]]
    if mk0 in ('block', 'html'):
        raws = [lines[i] for i in b]
        opens = ('/*' if mk0 == 'block' else '<!--')
        closes = ('*/' if mk0 == 'block' else '-->')
        n_open = sum(1 for r in raws if opens in r)
        n_close = sum(1 for r in raws if closes in r)
        if n_open > 1 or n_close > 1:
            return '嵌套块护栏'
        if closes not in raws[-1]:
            return '未闭合护栏'
    return None


def join_parts(parts):
    """合并为一段：汉字↔汉字、CJK 标点贴邻不加空格；中西文边界补空格
    （与本仓既有文风一致：『对空 vec 取下标』、『（docs/56）改 rem』）。"""
    out = ''
    for p in parts:
        if not out:
            out = p
            continue
        a, b = out[-1], p[:1]
        if (CJK_LETTER.match(a) and CJK_LETTER.match(b)) \
                or CJK_PUNCT.match(a) or CJK_PUNCT.match(b):
            out += p
        else:
            out += ' ' + p
    return out


def wrap_two(text, budget):
    """把 text 排成 ≤2 行；返回行列表。"""
    if len(text) <= budget:
        return [text]
    mid = len(text) // 2
    cut = text.rfind(' ', 0, mid + 20)
    if cut <= 0:
        cut = mid
    l1, l2 = text[:cut].rstrip(), text[cut:].lstrip()
    return [l1, l2] if l2 else [l1]


def transform_batch2(lines, kinds, contents, markers, lang):
    n = len(lines)
    first_code = S.first_code_index(kinds, lines)
    blocks, cur = [], []
    for i, k in enumerate(kinds):
        if k == 'comment':
            cur.append(i)
        else:
            if cur:
                blocks.append(cur)
            cur = []
    if cur:
        blocks.append(cur)

    stats = {'blocks': 0, 'lines_before': 0, 'lines_after': 0, 'skips': {}, 'guarded': 0}
    delete = [False] * n
    replace = {}

    for b in blocks:
        if len(b) < 3:
            continue
        reason = block_skip_reason(b, lines, kinds, contents, markers, lang, first_code)
        if reason:
            stats['skips'][reason] = stats['skips'].get(reason, 0) + 1
            continue
        mk = markers[b[0]]
        parts = [contents[i].strip() for i in b if contents[i].strip()]
        if not parts:
            continue
        indent = lines[b[0]][:len(lines[b[0]]) - len(lines[b[0]].lstrip())]
        merged = join_parts(parts)
        budget = MAXCOL - len(indent) - (3 if mk == 'line' else 7)
        wrapped = wrap_two(merged, budget)
        # 形态化
        if mk == 'line':
            new_lines = [f'{indent}// {t}' for t in wrapped]
        elif mk == 'pound':
            new_lines = [f'{indent}# {t}' for t in wrapped]
        elif mk == 'doc':
            new_lines = [f'{indent}/// {t}' for t in wrapped]
        elif mk == 'inner':
            new_lines = [f'{indent}//! {t}' for t in wrapped]
        elif mk == 'html':
            if len(wrapped) == 1:
                new_lines = [f'{indent}<!-- {wrapped[0]} -->']
            else:
                new_lines = [f'{indent}<!-- {wrapped[0]}',
                             f'{indent}{wrapped[1]} -->']
        else:  # /* */ 块
            if len(wrapped) == 1:
                new_lines = [f'{indent}/* {wrapped[0]} */']
            else:
                # 续行与开符行同列对齐——rustfmt 对块注释续行缩进有此要求
                new_lines = [f'{indent}/* {wrapped[0]}',
                             f'{indent}{wrapped[1]} */']
        for i in b:
            delete[i] = True
        replace[b[0]] = new_lines
        stats['blocks'] += 1
        stats['lines_before'] += len(b)
        stats['lines_after'] += len(new_lines)

    if not any(delete):
        return lines, stats
    out = []
    for i in range(n):
        if i in replace:
            out.extend(replace[i])
        elif not delete[i]:
            out.append(lines[i])
    return out, stats


# ---------------------------------------------------------------- 第三批：剥引用（docs/60）

# 引用单向化：注释里剥掉指向 docs 的引用，保留现行理由。顺序敏感，(模式, 替换)。
STRIP_PATTERNS = [
    # 行首标签形态：docs/28：xxx / docs/24 B5b 验收：xxx（含「（docs/28：」括号内起头，
    # 连同行尾收括号一并吃掉，避免剥完留下孤儿「）」；内容经捕获组保留）
    (re.compile(r'^\s*docs/\d+[^：\n]{0,24}：\s*'), ''),
    (re.compile(r'^\s*[（(]\s*docs/\d+[^：\n]{0,24}：\s*([^）]*)[）)]'), r'\1'),
    # 日期 parenthetical：日期是历史标记（语域禁令），理由文字保留
    (re.compile(r'[（(]\s*20\d{2}-\d{2}-\d{2}[^（）]{0,40}[）)]'), ''),
    (re.compile(r'^\s*20\d{2}-\d{2}-\d{2}\s*用户?(?:反馈|拍板|报)?[^：\n]{0,12}[：:]?\s*'), ''),
    (re.compile(r'20\d{2}-\d{2}-\d{2}\s*用户?(?:反馈|拍板|报)?[：:]?\s*'), ''),
    # 短括号引用整体删（内文 ≤16 字，全是出处与连接语）
    (re.compile(r'[（(]\s*(?:见\s*|详见\s*|参见\s*)?docs/\d[^（）]{0,16}[）)]'), ''),
    (re.compile(r'[（(]\s*(?:见\s*)?ADR-\d+[^（）]{0,16}[）)]'), ''),
    # 引用 token（含小节/批次/点号限定词），不动所在括号的其余内容。
    # 注意：docs/\d 后不得用 \S* —— 会把「）。」这类正文标点一起吞掉。
    (re.compile(r'\s*(?:见\s*|详见\s*|参见\s*)?docs/\d+'
                r'(?:\s*(?:§[\d.]+|批\s*[A-D]|P\d-\d|G\d|D\d|B\d+\w?|[A-Z]\d(?:-\d+)?'
                r'|第\s*[\d一二三四五六七八九十百]+\s*点?|[\d一二三四五六七八九十百]+轮'
                r'|决策\s*\d+))*'), ''),
    (re.compile(r'\s*(?:见\s*)?ADR-\d+'), ''),
    (re.compile(r'\s*(?:见\s*)?(?:notes/\S*|AGENTS\.md)(?:\s*§[\d.]+)?'), ''),
]
STRIP_CLEANUPS = [
    (re.compile(r'[（(]\s*[）)]'), ''),                    # 空括号
    (re.compile(r'(?:见|详见|参见)\s*[）)]'), '）'),
    (re.compile(r'[，、]\s*[）)]'), '）'),
    (re.compile(r'[（(][：；、，]\s*'), '（'),
    (re.compile(r'[（(]的'), '（'),
    (re.compile(r'[（(]决策\s*\d+[：:]?\s*'), '（'),
    (re.compile(r'^\s*[）)]'), ''),                        # 行首悬闭括号
    (re.compile(r'[（(]\s+'), '（'),
    (re.compile(r'\s+[）)]'), '）'),
    (re.compile(r'\s+([，。；：、])'), r'\1'),
    (re.compile(r'([，。；：、])\1+'), r'\1'),
    (re.compile(r'——\s*$'), ''),
    (re.compile(r'——\s*，'), '，'),
    (re.compile(r'^[\s，。；：、——-]+'), ''),
    (re.compile(r'[\s，、—-]+$'), ''),
    (re.compile(r'\s{2,}'), ' '),
]


def strip_citations(c):
    """剥掉内容里的 docs 引用与日期戳，保留其余文字；无命中时原样返回。"""
    if not re.search(r'docs/\d|notes/|ADR-\d|AGENTS\.md|20\d{2}-\d{2}-\d{2}', c):
        return c
    out = c
    for rx, rep in STRIP_PATTERNS:
        out = rx.sub(rep, out)
    for rx, rep in STRIP_CLEANUPS:
        out = rx.sub(rep, out)
    return out


def transform_batch3(lines, kinds, contents, markers, lang, rel, plan=None):
    """剥引用 + 删剥后变空的纯指针行 + 应用人工计划（rewrites/deletes）。"""
    n = len(lines)
    plan = plan or {}
    rewrites = {(r['file'], r['line']): r['text'] for r in plan.get('rewrites', [])}
    del_lines = defaultdict(list)
    for d in plan.get('deletes', []):
        del_lines[d['file']].extend(d['lines'])
    planned_del = set(del_lines.get(rel, []))

    delete = [False] * n
    stats = {'stripped': 0, 'deleted': 0, 'planned_rewrite': 0, 'planned_delete': 0,
             'leftover_refs': 0, 'guarded': 0}
    leftovers = []
    for i in range(n):
        if kinds[i] != 'comment':
            continue
        mk = markers[i]
        # L2 契约块延后；操作性行不碰
        if mk in ('doc', 'inner'):
            continue
        if lines[i].lstrip().startswith(('/**', '/*!')):
            continue
        if re.match(r'^\s*#\s*-\*-', lines[i]) or (i < 2 and re.search(r'coding[:=]', contents[i])):
            continue
        key = (rel, i + 1)
        if key in rewrites:
            new_text = rewrites[key]
            new = rewrite_comment_line(lines[i], mk, new_text,
                                       old_content=None) if mk in ('line', 'pound') else None
            if new is None:
                # 块注释行：按旧内容逐字替换
                if contents[i].strip() in lines[i]:
                    new = lines[i].replace(contents[i].strip(), new_text, 1)
            if new is not None:
                lines[i] = new
                contents[i] = new_text
                stats['planned_rewrite'] += 1
                continue
        if i + 1 in planned_del:
            if not block_line_editable(lines[i], mk):
                stats['guarded'] = stats.get('guarded', 0) + 1
                continue
            delete[i] = True
            stats['planned_delete'] += 1
            continue
        c = contents[i]
        new_c = strip_citations(c)
        if new_c == c:
            continue
        new_c = new_c.strip()
        if not new_c.strip(' \t*') or not re.search(r'[\u4e00-\u9fff\w]', new_c):
            # 剥完只剩标点/星号 → 纯指针行，删除
            if not block_line_editable(lines[i], mk):
                stats['guarded'] += 1
                leftovers.append(f'{rel}:{i+1} 纯指针但处块开/闭符，需人工')
                continue
            delete[i] = True
            stats['deleted'] += 1
            continue
        new = rewrite_comment_line(lines[i], mk, new_c, old_content=c.strip())
        if new is None:
            leftovers.append(f'{rel}:{i+1} 无法重写（块形态），需人工')
            continue
        # 括号平衡的「增量」不变 = 跨行括号的原有形态，不算破损
        bal_o = c.count('（') - c.count('）') + c.count('(') - c.count(')')
        bal_n = new_c.count('（') - new_c.count('）') + new_c.count('(') - new_c.count(')')
        broken = bal_o != bal_n or \
            re.search(r'（\s*[，。；：、）]|（$|[，：；]\s*）|）\s*（', new_c)
        if broken:
            # 破损不落盘，留人工计划处理
            stats['leftover_refs'] += 1
            leftovers.append(f'{rel}:{i+1} 剥后文本破损: {new_c[:80]}')
            continue
        if re.search(r'docs/\d|notes/|ADR-\d|AGENTS\.md', new_c):
            stats['leftover_refs'] += 1
            leftovers.append(f'{rel}:{i+1} 剥后仍有引用: {new_c[:80]}')
            continue
        lines[i] = new
        contents[i] = new_c
        stats['stripped'] += 1

    out_lines = collapse_blanks(lines, delete, kinds)
    stats['leftovers'] = leftovers
    return out_lines, stats

def gate_line_diff(old_lines, new_lines, old_kinds, new_kinds):
    """门A：增删行必须都是 blank/comment。返回违规列表。"""
    bad = []
    sm = difflib.SequenceMatcher(None, old_lines, new_lines, autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag == 'equal':
            continue
        for i in range(i1, i2):
            ln, k = old_lines[i], old_kinds[i]
            if k in ('blank', 'comment'):
                continue
            bad.append(('del', i + 1, k, ln))
        for j in range(j1, j2):
            ln, k = new_lines[j], new_kinds[j]
            if k in ('blank', 'comment'):
                continue
            bad.append(('add', j + 1, k, ln))
    # 属性行显式拦截（试跑批教训：属性行不是注释，删了不报错）
    for side, i, k, ln in list(bad):
        if re.match(r'^\s*#\[', ln):
            bad.append(('ATTR', i, k, ln))
    return bad


def gate_attr_count(old_lines, new_lines):
    """门B：#[...] 属性行总数不变。"""
    o = sum(1 for l in old_lines if re.match(r'^\s*#\[', l))
    w = sum(1 for l in new_lines if re.match(r'^\s*#\[', l))
    return o, w


def gate_strip_compare(old_lines, old_kinds, new_lines, new_kinds):
    """门C：剥注释 + 丢空行后逐行一致。"""
    so = [l for l, k in zip(old_lines, old_kinds) if k in ('code', 'mixed')]
    sw = [l for l, k in zip(new_lines, new_kinds) if k in ('code', 'mixed')]
    if so == sw:
        return None
    diffs = []
    for idx, (a, b2) in enumerate(zip(so, sw)):
        if a != b2:
            diffs.append((idx, a, b2))
            if len(diffs) >= 3:
                break
    return (len(so), len(sw), diffs)


# ---------------------------------------------------------------- 主流程

def run_scope(scope, batch, dry, plan_path=None):
    dirs = {'rust': S.RUST_DIRS, 'frontend': S.FE_DIRS + S.TOOL_DIRS,
            'python': S.PY_DIRS}[scope]
    plan = None
    if plan_path:
        plan = json.loads(Path(plan_path).read_text(encoding='utf-8'))
    # 元工具豁免：comment-gov 自己的注释在讲"引用长什么样"，不是引用本体
    files = [p for p in S.iter_files(dirs)
             if not S.rel(p).startswith('tools/comment-gov/')]
    all_fail = False
    failures = []
    report = {'scope': scope, 'batch': batch, 'dry': dry, 'files': [], 'failures': []}
    planned = []

    for p in files:
        loaded = S.load_text(p)
        if not loaded:
            failures.append((S.rel(p), '无法解析（编码/混合换行）'))
            continue
        lines, eol, bom, trailing = loaded
        lang = S.LANG_BY_EXT[p.suffix]
        kinds, contents, markers, indents = S.analyze(lines, lang)

        if batch == 1:
            new_lines, stats = transform_batch1(list(lines), kinds, contents, markers)
        elif batch == 2:
            new_lines, stats = transform_batch2(list(lines), kinds, contents, markers, lang)
        else:
            new_lines, stats = transform_batch3(list(lines), kinds, contents, markers,
                                                lang, S.rel(p), plan)
        changed = new_lines != lines
        entry = {'file': S.rel(p), 'changed': changed, 'stats': stats}
        if stats.get('leftovers'):
            entry['leftovers'] = stats['leftovers']
        if changed:
            new_kinds, _, _, _ = S.analyze(new_lines, lang)
            gA = gate_line_diff(lines, new_lines, kinds, new_kinds)
            o, w = gate_attr_count(lines, new_lines)
            gC = gate_strip_compare(lines, kinds, new_lines, new_kinds)
            entry['gates'] = {'A': len(gA), 'B': (o, w), 'C': None if gC is None else True}
            if gA:
                entry['gA_detail'] = gA[:5]
                failures.append((S.rel(p), f'门A {len(gA)} 处违规'))
            if o != w:
                failures.append((S.rel(p), f'门B 属性行 {o} → {w}'))
            if gC is not None:
                failures.append((S.rel(p), f'门C 剥注释不一致 {gC[0]} vs {gC[1]}'))
            planned.append((p, eol, bom, trailing, lines, new_lines, entry))
        report['files'].append(entry)

    if failures:
        all_fail = True
        print(f'\n!! {scope} 第{batch}批：{len(failures)} 个文件未过门，全部不写盘')
        for f, why in failures[:20]:
            print(f'   {f}: {why}')
        for f, why in failures:
            report['failures'].append({'file': f, 'why': why})
    elif not dry:
        for p, eol, bom, trailing, old, new, entry in planned:
            body = (eol.join(new) + eol) if trailing else eol.join(new)
            data = body.encode('utf-8')
            if bom:
                data = b'\xef\xbb\xbf' + data
            p.write_bytes(data)
            entry['written'] = True

    chg = [e for e in report['files'] if e['changed']]
    tot = {}
    for e in report['files']:  # 统计含"只有拦截、无改动"的文件
        for k, v in e['stats'].items():
            if isinstance(v, int):
                tot[k] = tot.get(k, 0) + v
            elif isinstance(v, dict):
                for k2, v2 in v.items():
                    tot[k2] = tot.get(k2, 0) + v2
    tag = '（dry-run）' if dry else ('（已写盘）' if not all_fail else '（拦截，未写盘）')
    print(f'== {scope} 第{batch}批 {tag}：{len(chg)}/{len(report["files"])} 文件有改动')
    for k in sorted(tot):
        print(f'   {k}: {tot[k]}')

    REPORTS.mkdir(exist_ok=True)
    out = REPORTS / f'apply-{scope}-b{batch}{"-dry" if dry else ""}.json'
    out.write_text(json.dumps(report, ensure_ascii=False, indent=1, default=str),
                   encoding='utf-8')
    print(f'   报告：{out}')
    return 1 if all_fail else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--scope', choices=['rust', 'frontend', 'python'], required=True)
    ap.add_argument('--batch', type=int, choices=[1, 2, 3], required=True)
    ap.add_argument('--plan', help='batch 3 的人工计划 JSON（rewrites/deletes）')
    ap.add_argument('--dry', action='store_true')
    a = ap.parse_args()
    sys.exit(run_scope(a.scope, a.batch, a.dry, a.plan))


if __name__ == '__main__':
    main()
