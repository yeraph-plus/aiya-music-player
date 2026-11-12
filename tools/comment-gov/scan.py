#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""注释机械治理 · 扫描器（只读，不改文件）。

行分类口径（与 docs/60 §3 对齐）：
- blank   空行
- comment 纯注释行（整行都是注释；含块注释续行、svelte 标记里的 HTML 注释）
- code    代码行
- mixed   行尾注释行（算代码行，本治理永不触碰）

与 tools/check-style.mjs 的关键差别：本扫描器带完整状态机——
字符串、模板字符串（含 ${} 嵌套与 GLSL 内容）、Rust 原始字符串/字符/生命周期/
嵌套块注释、CSS 字符串、正则字面量里的内容一律不算注释（失败封闭：
拿不准就归为代码行，宁可漏删不可误删）。

注释块 = 连续纯注释行，空行或代码行打断（docs/60 §3）。
L1 文件头 = 首个非注释非空行之前的注释块。

用法：
    python -I tools/comment-gov/scan.py            # 全量报告
    python -I tools/comment-gov/scan.py --json     # 另存明细 JSON
"""

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = Path(__file__).resolve().parent / 'reports'

RUST_DIRS = ['crates', 'src-tauri']
FE_DIRS = ['ui/src', 'ui/e2e']
TOOL_DIRS = ['tools']
PY_DIRS = ['beets-tool/src', 'beets-tool/tests', 'beets-tool/tools']

# 目录名任一层级命中即跳过：生成物（C4 不手改）与构建产物
SKIP_COMPONENTS = {
    'node_modules', 'dist', 'target', 'test-results', 'generated',
    'binaries', 'gen', 'bindings', 'vendor', 'fixtures',
    '.runtime', 'runtime', 'third-party',
}

LANG_BY_EXT = {
    '.rs': 'rust', '.ts': 'js', '.mjs': 'js', '.js': 'js',
    '.svelte': 'svelte', '.html': 'svelte', '.css': 'css',
    '.py': 'python',
}

# ---------------------------------------------------------------- 文本装载

def load_text(path):
    """读文件 → (lines_without_eol, eol, had_bom, trailing_newline)；失败返回 None。"""
    return load_text_bytes(path.read_bytes())


def load_text_bytes(data):
    """bytes → (lines_without_eol, eol, had_bom, trailing_newline)；失败返回 None。"""
    had_bom = data.startswith(b'\xef\xbb\xbf')
    if had_bom:
        data = data[3:]
    try:
        text = data.decode('utf-8')
    except UnicodeDecodeError:
        return None
    crlf = text.count('\r\n')
    lone_cr = text.count('\r') - crlf
    lf = text.count('\n') - crlf
    if lone_cr or (crlf and lf):  # 混合换行：失败封闭，跳过
        return None
    eol = '\r\n' if crlf else '\n'
    body = text.replace('\r\n', '\n') if crlf else text
    lines = body.split('\n')
    trailing_newline = True
    if lines and lines[-1] == '':
        lines.pop()
    else:
        trailing_newline = False
    return lines, eol, had_bom, trailing_newline


def _py_raw_prefix(line, i):
    """i 处引号的字符串前缀是否含 r（原始字符串，转义不生效）。"""
    j = i
    while j > 0 and line[j - 1] in 'rbfuRBFU':
        j -= 1
    if j > 0 and line[j - 1] in WORD:
        return False  # 前缀属于更长标识符
    return any(c in 'rR' for c in line[j:i])


def first_code_index(kinds, lines):
    """首个代码/混合行。Python shebang（#!）不算代码——其后的 # 头块仍按 L1 保护。"""
    start = 1 if (lines and lines[0].lstrip().startswith('#!')) else 0
    for i in range(start, len(kinds)):
        if kinds[i] in ('code', 'mixed'):
            return i
    return len(kinds)


# ---------------------------------------------------------------- 状态机

WORD = set('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_$')
KW_BEFORE_REGEX = {
    'return', 'typeof', 'instanceof', 'in', 'of', 'new', 'delete', 'void',
    'throw', 'case', 'do', 'else', 'yield', 'await',
}
RAW_RE = re.compile(r'(?:b|r|br|cr)(#*)"')

# 行记录 kind：blank / comment / code / mixed
# comment 行附带：content（注释正文）、marker（line/block/html）、indent


def analyze(lines, lang):
    n = len(lines)
    kinds = ['code'] * n
    contents = [''] * n
    markers = [''] * n
    indents = [''] * n

    # svelte：按行切 section（markup / script / style）
    sec_of = None
    if lang == 'svelte':
        sec_of = []
        sec = 'markup'
        for ln in lines:
            s = ln.lstrip().lower()
            if s.startswith('<script'):
                sec = 'script'
            elif s.startswith('</script'):
                sec = 'markup'
            elif s.startswith('<style'):
                sec = 'style'
            elif s.startswith('</style'):
                sec = 'markup'
            sec_of.append(sec)

    # ---- js 状态 ----
    js_frames = []          # 元素：{'t':'code','d':n} 或 {'t':'tmpl'}
    js_block = 0            # /* */ 深度（不嵌套，>0 即在块内）
    js_str = None           # 引号字符
    js_regex = False        # 正在扫正则字面量
    js_re_class = False     # 正则内 [...] 字符类
    last_sig = None         # 上一非空白字符（跨行，用于正则判定）
    last_word = ''
    mk_expr_depth = None    # svelte 标记表达式深度（None=不在表达式内）
    # ---- rust 状态 ----
    rs_block = 0            # 嵌套块注释深度
    rs_raw = None           # 原始字符串井号数
    rs_str = False          # 普通字符串/字节字符串
    rs_char = False         # 字符字面量（跨行视为异常，行内处理）
    # ---- css 状态 ----
    css_block = 0
    css_str = None
    # ---- python 状态 ----
    py_q = None           # 当前字符串定界符（'"' 或 "'"），None=不在字符串内
    py_seq = None         # 三引号定界符（'"""' 或 "'''"）
    py_triple = False
    py_raw = False
    # ---- svelte 标记状态 ----
    html = False            # <!-- -->

    for idx, line in enumerate(lines):
        # svelte：按行切 section（markup / script / style）
        if lang == 'svelte':
            mode = sec_of[idx]
        elif lang == 'python':
            mode = 'python'
        else:
            mode = 'rust' if lang == 'rust' else ('css' if lang == 'css' else 'js')

        # 行首携带状态
        js_in_expr = mode == 'markup' and mk_expr_depth is not None
        carried_str = (
            rs_str is True or rs_raw is not None
            or ((lang == 'js' or mode == 'script' or js_in_expr)
                and (js_str is not None or js_regex
                     or (js_frames and js_frames[-1]['t'] == 'tmpl')))
        )
        carried_block = (
            (mode == 'rust' and rs_block > 0)
            or ((lang == 'js' or mode == 'script' or js_in_expr) and js_block > 0)
            or (mode == 'style' and css_block > 0)
            or (mode == 'markup' and html)
        )
        code_seen = carried_str
        comment_seen = carried_block
        pieces = []
        i, L = 0, len(line)
        indent = line[:len(line) - len(line.lstrip())]
        step_over_line = False

        while i < L:
            ch = line[i]

            # ============ rust ============
            if mode == 'rust':
                if rs_raw is not None:
                    if ch == '"' and line[i + 1:i + 1 + rs_raw] == '#' * rs_raw:
                        i += 1 + rs_raw
                        rs_raw = None
                        last_sig = '"'
                    else:
                        i += 1
                    continue
                if rs_str:
                    if ch == '\\':
                        i += 2
                        continue
                    if ch == '"':
                        rs_str = False
                        last_sig = '"'
                    i += 1
                    continue
                if rs_block > 0:
                    if line.startswith('/*', i):
                        rs_block += 1
                        i += 2
                        comment_seen = True
                        continue
                    if line.startswith('*/', i):
                        rs_block -= 1
                        i += 2
                        continue
                    comment_seen = True
                    pieces.append(ch)
                    i += 1
                    continue
                if ch == '/' and line.startswith('//', i):
                    comment_seen = True
                    pieces.append(line[i + 2:])
                    i = L
                    break
                if ch == '/' and line.startswith('/*', i):
                    rs_block += 1
                    comment_seen = True
                    i += 2
                    continue
                if ch == '"':
                    rs_str = True
                    code_seen = True
                    i += 1
                    continue
                if ch == "'":
                    if i + 1 < L and line[i + 1] == '\\':
                        # 字符字面量 '\n' 等：找到收尾引号
                        j = i + 2
                        while j < L and line[j] != "'":
                            j += 2 if line[j] == '\\' else 1
                        i = j + 1
                        code_seen = True
                        continue
                    if i + 2 < L and line[i + 2] == "'":
                        i += 3
                        code_seen = True
                        continue
                    # 生命周期 'a：跳过标识符
                    j = i + 1
                    while j < L and line[j] in WORD:
                        j += 1
                    i = j
                    code_seen = True
                    continue
                m = RAW_RE.match(line, i) if ch in 'bcr' else None
                if m and (i == 0 or line[i - 1] not in WORD):
                    if m.group(1):
                        rs_raw = len(m.group(1))
                    else:
                        rs_str = True
                    code_seen = True
                    i = m.end()  # match(line, i) 的 end 是绝对索引
                    continue
                if ch not in ' \t':
                    code_seen = True
                i += 1
                continue

            # ============ css ============
            if mode == 'style':
                if css_block > 0:
                    if line.startswith('*/', i):
                        css_block -= 1
                        i += 2
                        continue
                    comment_seen = True
                    pieces.append(ch)
                    i += 1
                    continue
                if line.startswith('/*', i):
                    css_block += 1
                    comment_seen = True
                    i += 2
                    continue
                if css_str:
                    if ch == '\\':
                        i += 2
                        continue
                    if ch == css_str:
                        css_str = None
                    i += 1
                    continue
                if ch in '"\'':
                    css_str = ch
                    code_seen = True
                    i += 1
                    continue
                if ch not in ' \t':
                    code_seen = True
                i += 1
                continue

            # ============ python ============
            if mode == 'python':
                if py_q is not None:
                    # 字符串内容（含 docstring——运行时值，永不按注释处理）
                    if py_triple:
                        if line.startswith(py_seq, i):
                            py_q = py_seq = None
                            py_triple = False
                            code_seen = True  # 闭合行是字符串定界，不是注释
                            last_sig = '"'
                            i += 3
                            continue
                    elif ch == py_q:
                        py_q = None
                        code_seen = True
                        last_sig = '"'
                        i += 1
                        continue
                    if ch == '\\':
                        i += 2
                        continue
                    code_seen = True
                    i += 1
                    continue
                if ch == '#':
                    if idx == 0 and line.startswith('#!'):
                        code_seen = True  # shebang 是操作性行，不参与变换
                        i = L
                        break
                    comment_seen = True
                    pieces.append(line[i + 1:])
                    i = L
                    break
                if line.startswith(('"""', "'''"), i):
                    py_seq = line[i:i + 3]
                    py_q = py_seq[0]
                    py_triple = True
                    py_raw = _py_raw_prefix(line, i)
                    code_seen = True
                    i += 3
                    continue
                if ch in '"\'':
                    py_q = ch
                    py_triple = False
                    py_raw = _py_raw_prefix(line, i)
                    code_seen = True
                    i += 1
                    continue
                if ch not in ' \t':
                    code_seen = True
                i += 1
                continue

            # ============ svelte 标记 ============
            if mode == 'markup' and mk_expr_depth is None:
                if ch in ' \t':
                    i += 1
                    continue
                if html:
                    j = line.find('-->', i)
                    if j == -1:
                        pieces.append(line[i:])
                        i = L
                        break
                    pieces.append(line[i:j])
                    html = False
                    i = j + 3
                    continue
                if line.startswith('<!--', i):
                    comment_seen = True
                    j = line.find('-->', i + 4)
                    if j == -1:
                        html = True
                        pieces.append(line[i + 4:])
                        i = L
                        break
                    pieces.append(line[i + 4:j])
                    i = j + 3
                    continue
                if ch == '{':
                    mk_expr_depth = 1   # 进入标记表达式，交给 js 分支扫描
                    last_sig = 'x'      # 防 {/if} 的 / 被判成正则
                    last_word = ''
                    code_seen = True
                    i += 1
                    continue
                code_seen = True
                i += 1
                continue

            # ============ js / ts ============
            frame = js_frames[-1] if js_frames else None
            if frame and frame['t'] == 'tmpl':
                if ch == '\\':
                    i += 2
                    continue
                if ch == '`':
                    js_frames.pop()
                    last_sig = '`'
                    i += 1
                    continue
                if line.startswith('${', i):
                    js_frames.append({'t': 'code', 'd': 0})
                    code_seen = True
                    i += 2
                    continue
                code_seen = True  # 模板字符串内容（含 GLSL）永不按注释处理
                i += 1
                continue
            if js_regex:
                if ch == '\\':
                    i += 2
                    continue
                if ch == '[':
                    js_re_class = True
                elif ch == ']':
                    js_re_class = False
                elif ch == '/' and not js_re_class:
                    js_regex = False
                    last_sig = '/'
                i += 1
                continue
            if js_block > 0:
                j = line.find('*/', i)
                if j == -1:
                    comment_seen = True
                    pieces.append(line[i:])
                    i = L
                    break
                comment_seen = True
                pieces.append(line[i:j])
                js_block = 0
                i = j + 2
                continue
            if js_str:
                if ch == '\\':
                    i += 2
                    continue
                if ch == js_str:
                    last_sig = js_str
                    js_str = None
                i += 1
                continue
            if ch == '}' and frame and frame['t'] == 'code' and frame['d'] == 0:
                js_frames.pop()  # 模板表达式结束，回到模板内容
                last_sig = '}'
                code_seen = True
                i += 1
                continue
            if mk_expr_depth is not None and ch in '{}':
                # svelte 标记表达式的花括号配对（模板 ${} 的花括号由 frame 自管）
                mk_expr_depth += 1 if ch == '{' else -1
                if mk_expr_depth == 0:
                    mk_expr_depth = None  # 表达式结束，余下字符回到标记模式
                last_sig = 'x'
                last_word = ''
                code_seen = True
                i += 1
                continue
            if ch in ' \t':
                i += 1
                continue
            if ch == '/' and line.startswith('//', i):
                comment_seen = True
                pieces.append(line[i + 2:])
                i = L
                break
            if ch == '/' and line.startswith('/*', i):
                js_block = 1
                comment_seen = True
                i += 2
                continue
            code_seen = True
            if ch == '{' and frame and frame['t'] == 'code':
                frame['d'] += 1
                last_sig = '{'
                i += 1
                continue
            if ch == '}' and frame and frame['t'] == 'code':
                frame['d'] -= 1
                last_sig = '}'
                i += 1
                continue
            if ch == '/':
                sig_allows = (
                    last_sig is None
                    or last_sig in '([{,;=:!&|?+-*%^~<>'
                    or (last_word in KW_BEFORE_REGEX)
                )
                if sig_allows:
                    js_regex = True
                    js_re_class = False
                    i += 1
                    continue
                last_sig = '/'
                last_word = ''
                i += 1
                continue
            if ch in '"\'':
                js_str = ch
                last_word = ''
                i += 1
                continue
            if ch == '`':
                js_frames.append({'t': 'tmpl'})
                last_word = ''
                i += 1
                continue
            if ch in WORD:
                last_word += ch if ch.isalnum() or ch == '_' else ''
                if not (ch.isalnum() or ch == '_'):
                    last_word = ''
            else:
                last_word = ''
            last_sig = ch
            i += 1

        # ---- 行终判 ----
        if not line.strip() and not comment_seen and not code_seen:
            kind = 'blank'
        elif code_seen:
            kind = 'mixed' if comment_seen else 'code'
        else:
            kind = 'comment'

        kinds[idx] = kind
        indents[idx] = indent
        if kind == 'comment' or (kind == 'mixed' and comment_seen):
            raw = ''.join(pieces).strip()
            if mode == 'rust' and line.lstrip().startswith('///'):
                markers[idx] = 'doc'
            elif mode == 'rust' and line.lstrip().startswith('//!'):
                markers[idx] = 'inner'
            elif mode == 'python':
                markers[idx] = 'pound'
            elif mode == 'markup' and html:
                markers[idx] = 'html'
            else:
                markers[idx] = 'block' if not line.lstrip().startswith('//') else 'line'
            if markers[idx] == 'doc':
                raw = re.sub(r'^/+ ?', '', raw)   # /// 的第三个斜杠不属正文
            elif markers[idx] == 'inner':
                raw = re.sub(r'^[!/]+ ?', '', raw)  # //! 的感叹号不属正文
            contents[idx] = re.sub(r'^\*+\s?', '', raw)

    return kinds, contents, markers, indents


# ---------------------------------------------------------------- 分类规则

SEPARATOR_CHARS = set('=-_*~#·•—―+/.\\|─━～ ')
DATE_PATTERNS = {
    'iso': re.compile(r'20\d{2}-\d{2}-\d{2}'),
    'slash': re.compile(r'20\d{2}/\d{1,2}/\d{1,2}'),
    'dot': re.compile(r'20\d{2}\.\d{1,2}\.\d{1,2}'),
    'cn': re.compile(r'20\d{2}年\d{1,2}月'),
    'at': re.compile(r'@date\b|Date:', re.IGNORECASE),
}
EVIDENCE_RE = re.compile(
    r'docs/\d|notes/|AGENTS\.md|§|ADR-\d|\bC\d+\b'
    r'|test_\w|tests/|cargo test|svelte-check|include_str'
    r'|实测|验证：|证据|复现|基准|schema|sha256|PROTOCOL_VERSION|\.rs:\d'
)
MARKER_RE = re.compile(r'^(契约：|场景：)')
TABLE_RE = re.compile(r'\|.*\||^\s*\+[-=+]+\+', re.MULTILINE)
LIST_RE = re.compile(r'^\s*(-\s|\*\s|•\s|\d+\.\s|\[ \]|\[x\])')
FENCE = '```'


def is_separator_content(text):
    s = text.strip()
    if len(s) < 4 or any(c.isalnum() for c in s):
        return False
    if '|' in s:
        return False  # 含 | 的"纯符号行"是 markdown 表格分隔行，不是装饰
    return all(c in SEPARATOR_CHARS for c in s)


def is_empty_content(text):
    return text.strip(' \t*') == ''


def rel(path):
    return path.relative_to(ROOT).as_posix()


def iter_files(dirs):
    out = []
    for d in dirs:
        base = ROOT / d
        if not base.exists():
            continue
        for p in sorted(base.rglob('*')):
            if not p.is_file():
                continue
            if set(p.relative_to(ROOT).parts) & SKIP_COMPONENTS:
                continue
            if p.suffix in LANG_BY_EXT:
                out.append(p)
    return out


# ---------------------------------------------------------------- 主流程

def scan_scope(name, dirs):
    files = iter_files(dirs)
    stats = Counter()
    batch1 = {'sep_blocks': [], 'date_lines': [], 'empty_blocks': []}
    batch2 = {'blocks': [], 'skips': Counter()}
    bad_files = []
    total_comment_lines = 0

    for p in files:
        loaded = load_text(p)
        if loaded is None:
            bad_files.append(rel(p))
            continue
        lines, eol, bom, trailing = loaded
        lang = LANG_BY_EXT[p.suffix]
        kinds, contents, markers, indents = analyze(lines, lang)
        total_comment_lines += sum(1 for k in kinds if k == 'comment')

        # 注释块切分
        blocks = []
        cur = []
        first_code = first_code_index(kinds, lines)
        for i, k in enumerate(kinds):
            if k == 'comment':
                cur.append(i)
            else:
                if cur:
                    blocks.append(cur)
                cur = []
        if cur:
            blocks.append(cur)

        for b in blocks:
            is_l1 = b[-1] < first_code
            all_rust_doc = lang == 'rust' and all(markers[i] == 'doc' for i in b)
            all_inner = lang == 'rust' and all(markers[i] == 'inner' for i in b)
            contents_b = [contents[i] for i in b]

            # ---------- 第一批 ----------
            if all(is_separator_content(c) for c in contents_b):
                batch1['sep_blocks'].append({'file': rel(p), 'lines': [i + 1 for i in b]})
                continue  # 纯分隔线块不参与第二批
            if all(is_empty_content(c) for c in contents_b):
                batch1['empty_blocks'].append({'file': rel(p), 'lines': [i + 1 for i in b]})
                continue
            for i in b:
                hit = [tag for tag, rx in DATE_PATTERNS.items() if rx.search(contents[i])]
                if hit:
                    batch1['date_lines'].append({
                        'file': rel(p), 'line': i + 1, 'tags': hit,
                        'text': contents[i].strip()[:100],
                        'doc_ref': bool(re.search(r'docs/\d|notes/', contents[i])),
                    })

            # ---------- 第二批 ----------
            if len(b) < 3:
                continue
            entry = {'file': rel(p), 'start': b[0] + 1, 'n': len(b),
                     'first': contents_b[0][:60], 'lang_scope': name}
            skip = None
            if is_l1 or all_inner:
                skip = 'L1文件头'
            elif all_rust_doc:
                skip = 'L2文档注释'
            elif MARKER_RE.match(contents_b[0].lstrip()):
                skip = '契约场景标记'
            elif FENCE in '\n'.join(contents_b):
                skip = '代码围栏'
            elif TABLE_RE.search('\n'.join(contents_b)):
                skip = '表格结构'
            elif any(EVIDENCE_RE.search(c) for c in contents_b):
                skip = '证据引用'
            elif any(LIST_RE.match(c) for c in contents_b):
                skip = '列表结构'
            if skip:
                batch2['skips'][skip] += 1
            else:
                batch2['blocks'].append(entry)

    return {
        'name': name,
        'files': len(files),
        'comment_lines': total_comment_lines,
        'bad_files': bad_files,
        'batch1': batch1,
        'batch2': batch2,
    }


# ---------------------------------------------------------------- 常驻门禁
# --check / --emit-allowlist：beets-tool Python 侧的注释文风门禁（docs/71 洞④）。
# 口径与 tools/check-style.mjs 对齐：日期戳、docs 引用与历史叙事信号词在
# 任何注释形态都是阻断，存量走 tools/check-style-allowlist.json 棘轮豁免
# （只减不增）。docstring 在机械批里是运行时值（apply 永不触碰），
# 在这里是注释面——它是收割主标的，门禁必须看见。

CHECK_DATE = re.compile(
    r'20\d{2}-\d{2}(-\d{2})?|20\d{2}年|\d{4}年'
    r'|20\d{2}/\d{1,2}/\d{1,2}|20\d{2}\.\d{1,2}\.\d{1,2}|@date\b|Date:',
    re.IGNORECASE,
)
CHECK_DOCREF = re.compile(r'docs/\d|notes/|ADR-\d|AGENTS\.md')
CHECK_NARRATIVE = re.compile(r'旧实现|此前|曾经|以前|旧版|事故')
ALLOWLIST_PATH = ROOT / 'tools' / 'check-style-allowlist.json'


def py_surface_segments(lines):
    """逐行返回注释面片段 [(text, col)]。

    注释面 = # 注释与行尾其后的全部内容、三引号字符串的内部文本。失败封闭：
    单引号串内部（含转义）按代码跳过；三引号转义定界符等罕见形态拿不准就
    按注释面处理——误报走白名单豁免，漏报等于门禁失效。
    """
    out = []
    triple = None  # 当前定界符：'"""' / "'''"
    for line in lines:
        segs = []
        i = 0
        n = len(line)
        if triple is not None:
            end = line.find(triple)
            if end == -1:
                segs.append((line, 0))
                out.append(segs)
                continue
            segs.append((line[:end], 0))
            i = end + len(triple)
            triple = None
        while i < n:
            c = line[i]
            if c == '#':
                segs.append((line[i + 1:], i + 1))
                break
            if c in '\'"':
                q3 = line[i:i + 3]
                if q3 in ('"""', "'''"):
                    close = line.find(q3, i + 3)
                    if close == -1:
                        segs.append((line[i + 3:], i + 3))
                        triple = q3
                        break
                    segs.append((line[i + 3:close], i + 3))
                    i = close + 3
                    continue
                j = i + 1
                while j < n:
                    if line[j] == '\\':
                        j += 2
                        continue
                    if line[j] == c:
                        break
                    j += 1
                i = j + 1 if j < n else n
                continue
            i += 1
        out.append(segs)
    return out


def py_check_scan():
    """beets-tool 全量扫描 → (blocking, hits, narrative, files_n)。

    blocking/hits 的元素：{rel, line, kind, tag, text}；blocking 是未豁免
    子集。豁免条目："相对路径"（整文件）或 "相对路径:行号"。
    """
    allowlist = []
    if ALLOWLIST_PATH.exists():
        allowlist = json.loads(ALLOWLIST_PATH.read_text(encoding='utf-8'))
    hits, narrative, files_n = [], 0, 0
    for p in iter_files(PY_DIRS):
        loaded = load_text(p)
        if loaded is None:
            continue
        lines = loaded[0]
        files_n += 1
        r = rel(p)
        for idx, segs in enumerate(py_surface_segments(lines)):
            for text, _col in segs:
                for rx, kind in ((CHECK_DATE, 'date'), (CHECK_DOCREF, 'docref'),
                                 (CHECK_NARRATIVE, 'narrative')):
                    m = rx.search(text)
                    if m:
                        hits.append({'rel': r, 'line': idx + 1, 'kind': kind,
                                     'tag': m.group(0), 'text': text.strip()[:90]})
                        if kind == 'narrative':
                            narrative += 1
    blocking = [h for h in hits
                if h['rel'] not in allowlist
                and f"{h['rel']}:{h['line']}" not in allowlist]
    return blocking, hits, narrative, files_n


def pycheck_main():
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    blocking, hits, narrative, files_n = py_check_scan()
    if '--emit-allowlist' in sys.argv:
        allowlist = json.loads(ALLOWLIST_PATH.read_text(encoding='utf-8')) \
            if ALLOWLIST_PATH.exists() else []
        merged = sorted(set(allowlist) | {f"{h['rel']}:{h['line']}" for h in hits})
        ALLOWLIST_PATH.write_text(
            json.dumps(merged, ensure_ascii=False, indent=1) + '\n', encoding='utf-8')
        print(f"comment-gov: 白名单合并 {len(merged) - len(set(allowlist))} 条，"
              f"共 {len(merged)} 条（棘轮基线，与 check-style 共用同一文件）")
        return
    for h in blocking:
        print(f"{h['rel']}:{h['line']}  [{h['kind']}/{h['tag']}]  {h['text']}")
    exempted = len(hits) - len(blocking)
    verdict = '通过' if not blocking else f'未通过（{len(blocking)} 处未豁免）'
    print(f"\ncomment-gov check: python 日期/引用/叙事词 {len(hits)}（豁免 {exempted}）"
          f"，扫描 {files_n} 个文件 —— {verdict}")
    if blocking:
        sys.exit(1)


def main():
    import sys as _sys
    _sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    scopes = {
        'rust': scan_scope('rust', RUST_DIRS),
        'frontend': scan_scope('frontend', FE_DIRS),
        'tools': scan_scope('tools', TOOL_DIRS),
        'python': scan_scope('python', PY_DIRS),
    }
    grand = Counter()
    lines_out = []
    for name, s in scopes.items():
        b1, b2 = s['batch1'], s['batch2']
        sep_lines = sum(len(x['lines']) for x in b1['sep_blocks'])
        emp_lines = sum(len(x['lines']) for x in b1['empty_blocks'])
        dates = len(b1['date_lines'])
        doc_ref = sum(1 for x in b1['date_lines'] if x['doc_ref'])
        b2_lines = sum(x['n'] for x in b2['blocks'])
        print(f"\n===== {name}（{s['files']} 文件，纯注释行 {s['comment_lines']}）=====")
        print(f"  第一批：纯分隔线块 {len(b1['sep_blocks'])} 块 / {sep_lines} 行；"
              f"空注释块 {len(b1['empty_blocks'])} 块 / {emp_lines} 行；"
              f"日期戳行 {dates}（其中含 docs 引用 {doc_ref}）")
        if s['bad_files']:
            print(f"  !! 无法解析（已跳过）：{s['bad_files']}")
        print(f"  第二批：可压缩内部多行块 {len(b2['blocks'])} 块 / {b2_lines} 行")
        for k, v in sorted(b2['skips'].items()):
            print(f"     跳过-{k}: {v} 块")
        grand['sep_blocks'] += len(b1['sep_blocks']); grand['sep_lines'] += sep_lines
        grand['emp_blocks'] += len(b1['empty_blocks']); grand['emp_lines'] += emp_lines
        grand['dates'] += dates; grand['doc_ref'] += doc_ref
        grand['b2_blocks'] += len(b2['blocks']); grand['b2_lines'] += b2_lines

    print('\n===== 合计 =====')
    print(f"  第一批将删：分隔线 {grand['sep_lines']} 行 + 空块 {grand['emp_lines']} 行"
          f" + 日期戳 {grand['dates']} 行（docs 引用 {grand['doc_ref']}）")
    print(f"  第二批可压缩：{grand['b2_blocks']} 块 / {grand['b2_lines']} 行"
          f"（压缩后 ≤2 行/块，预计减 ~{grand['b2_lines'] - grand['b2_blocks'] * 2} 行以上）")

    if '--json' in sys.argv:
        OUT_DIR.mkdir(exist_ok=True)
        payload = {name: {'batch1': s['batch1'], 'batch2': {'blocks': s['batch2']['blocks'],
                   'skips': dict(s['batch2']['skips'])}} for name, s in scopes.items()}
        (OUT_DIR / 'scan-detail.json').write_text(
            json.dumps(payload, ensure_ascii=False, indent=1), encoding='utf-8')
        print(f"\n明细已写：{OUT_DIR / 'scan-detail.json'}")

    # 日期戳行抽样（供口径核对）
    all_dates = [d for s in scopes.values() for d in s['batch1']['date_lines']]
    print('\n===== 日期戳行样例（前 25 行）=====')
    for d in all_dates[:25]:
        print(f"  {d['file']}:{d['line']}  {d['text']}")


if __name__ == '__main__':
    if '--check' in sys.argv or '--emit-allowlist' in sys.argv:
        pycheck_main()
    else:
        main()
