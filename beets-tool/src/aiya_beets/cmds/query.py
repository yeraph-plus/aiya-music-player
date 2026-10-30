"""索引查询与字段清单：`query` / `fields` 两条信封命令（docs/44 §5.1）。

## 两条命令为什么都不带 target

"查哪个库"由**进程**的 `AIYA_BEETS_PROFILE` 决定（批 1 的双家隔离）。
协议里再放一个 target 字段就是第二个真源：宿主传 `index`、进程跑在
`workbench` 上时会静默查出另一个库的全部内容——这种错不会抛异常，
只会让用户看见"索引里有几万首我没导过的歌"。

## 结构化规则为什么在工具侧求值，而不是拼成 beets 查询串

beets 的 `a..b` 范围比较只对在 `item_types` 里**注册过类型**的字段成立：
未注册的 flexible 字段存的是字符串，同样的范围查会**静默 0 命中**（实测：
自己声明 `types.Float` 的字段 3 命中，未声明的同值 0 命中）。把正确性押在
第三方插件有没有记得 register 上，等于让"有没有数据"变成运气。

`raw` 是保留的逃逸口，走 beets 自己的解析器——那边的语义就是 beets 的语义，
包括上面这条限制。两者分工写在这里，免得以后有人"顺手统一"。
"""

from __future__ import annotations

import itertools
import re
import sqlite3
from pathlib import Path

from ..beetslib import open_library
from ..protocol import E_INVALID_ARG, ProtocolError
from .enrich import source_table

_STAGE = "query"

# 支持的算子（与协议 QueryRule.op 的枚举一字不差）。
# 单独列一份是为了在**校验阶段**就能拒掉笔误：求值阶段拒绝的话，
# 空库上一辈子都不会触发出错。
OPS = frozenset({
    "eq", "ne", "lt", "lte", "gt", "gte",
    "contains", "prefix", "regex", "in", "range",
})

# 回带字段的默认值：UI 列表至少要能显示"这是哪首歌"。
DEFAULT_FIELDS = ("title", "artist", "album")

DEFAULT_LIMIT = 500
MAX_LIMIT = 5000

# beets 的类型系统 → 协议里的粗粒度类型（只用于 UI 渲染输入控件）
_TYPE_NAMES = {
    "Integer": "integer",
    "PaddedInt": "integer",
    "Float": "float",
    "String": "string",
    "Boolean": "bool",
    "DateType": "date",
    "DurationType": "duration",
}


def _as_number(v):
    """能转成 float 就转（bool 除外：True 不是 1，那是 Python 的坑）。

    flexible 字段从 SQLite 出来是 str / int / float 混着的，不做这一步
    就会出现 "0.55" < "0.9" 这类字符串比较——数值维度直接失真。
    """
    if isinstance(v, bool) or v is None:
        return None
    if isinstance(v, (int, float)):
        return float(v)
    try:
        return float(str(v).strip())
    except (TypeError, ValueError):
        return None


def _as_text(v) -> str:
    if isinstance(v, bytes):
        return v.decode("utf-8", "replace")
    return "" if v is None else str(v)


def _match(item, rule: dict) -> bool:
    """一条规则是否命中。**未知字段恒不命中**（不是报错）：flexible 键集合随
    插件装没装而变，拿它当门禁会制造"昨天的规则今天报错"。"""
    field = rule["field"]
    op = rule["op"]
    want = rule.get("value")
    try:
        got = item.get(field)
    except Exception:  # noqa: BLE001 —— 未知字段在 beets 里可能直接抛
        return False
    if got is None:
        return False

    if op in ("lt", "lte", "gt", "gte", "range"):
        a, b = _as_number(got), None
        if a is None:
            return False
        if op == "range":
            if not isinstance(want, list) or len(want) != 2:
                raise ProtocolError(E_INVALID_ARG, _STAGE, "range 的 value 必须是 [lo, hi]")
            lo, hi = _as_number(want[0]), _as_number(want[1])
            if lo is None or hi is None:
                return False
            return lo <= a <= hi
        b = _as_number(want)
        if b is None:
            return False
        if op == "lt":
            return a < b
        if op == "lte":
            return a <= b
        if op == "gt":
            return a > b
        return a >= b

    if op == "in":
        if not isinstance(want, list):
            raise ProtocolError(E_INVALID_ARG, _STAGE, "in 的 value 必须是数组")
        gn = _as_number(got)
        for w in want:
            if gn is not None and _as_number(w) == gn:
                return True
            if _as_text(got).lower() == _as_text(w).lower():
                return True
        return False

    if op in ("eq", "ne"):
        gn, wn = _as_number(got), _as_number(want)
        if gn is not None and wn is not None:
            hit = gn == wn
        else:
            hit = _as_text(got).lower() == _as_text(want).lower()
        return hit if op == "eq" else not hit

    text = _as_text(got).lower()
    needle = _as_text(want).lower()
    if op == "contains":
        return needle in text
    if op == "prefix":
        return text.startswith(needle)
    if op == "regex":
        try:
            return re.search(_as_text(want), _as_text(got), re.IGNORECASE) is not None
        except re.error as e:
            # 正则写错是宿主 bug，该立刻失败而不是"筛出零条"
            raise ProtocolError(E_INVALID_ARG, _STAGE, f"正则无效：{e}") from None
    raise ProtocolError(E_INVALID_ARG, _STAGE, f"未知算子：{op!r}")


def _check_rules(rules) -> list[dict]:
    if not isinstance(rules, list):
        raise ProtocolError(E_INVALID_ARG, _STAGE, "rules 必须是数组")
    out = []
    for r in rules:
        if not isinstance(r, dict):
            raise ProtocolError(E_INVALID_ARG, _STAGE, "rule 必须是对象")
        for k in ("field", "op"):
            if not isinstance(r.get(k), str) or not r[k]:
                raise ProtocolError(E_INVALID_ARG, _STAGE, f"rule 缺 {k}")
        if r["op"] not in OPS:
            # 在**校验阶段**就拒：等到逐条求值时才发现，库为空时这个错永远不会暴露
            raise ProtocolError(E_INVALID_ARG, _STAGE, f"未知算子：{r['op']!r}")
        if "value" not in r:
            raise ProtocolError(E_INVALID_ARG, _STAGE, "rule 缺 value")
        out.append({"field": r["field"], "op": r["op"], "value": r["value"]})
    return out


def _payload(item, fields: list[str]) -> dict:
    path = item.path.decode("utf-8", "replace")
    got = {}
    for f in fields:
        v = item.get(f)
        if isinstance(v, bytes):
            v = v.decode("utf-8", "replace")
        got[f] = v
    return {"id": item.id, "path": path, "fields": got}


def query(rules, raw, fields, limit) -> dict:
    """返回 `{items, total, truncated}`。`total` 是**应用 limit 前**的真实命中数。"""
    if rules and raw:
        raise ProtocolError(E_INVALID_ARG, _STAGE, "rules 与 raw 只能给一个（不猜优先级）")
    if raw is not None and not isinstance(raw, str):
        raise ProtocolError(E_INVALID_ARG, _STAGE, "raw 必须是字符串")
    if raw is not None and not raw.strip():
        raise ProtocolError(E_INVALID_ARG, _STAGE, "raw 为空字符串")
    if limit is None:
        limit = DEFAULT_LIMIT
    if not isinstance(limit, int) or isinstance(limit, bool) or not (1 <= limit <= MAX_LIMIT):
        raise ProtocolError(E_INVALID_ARG, _STAGE, f"limit 必须在 1..{MAX_LIMIT}")

    want_fields = [str(f) for f in (fields or DEFAULT_FIELDS) if str(f).strip()]

    lib = open_library()
    if raw is not None:
        try:
            results = lib.items(raw.strip())
            # `len()` 走的是 `SELECT COUNT(*)`，不反序列化任何 item。
            # 写 `len(list(results))` 会把整库读进内存再取长度——几万首时
            # 是几百 MB 的瞬时峰值，只为得到一个数字。
            total = len(results)
            page = list(itertools.islice(results, limit))
        except Exception as e:  # noqa: BLE001 —— 查询语法错误要让用户看见原文
            raise ProtocolError(E_INVALID_ARG, _STAGE, f"查询语法无效：{e}") from None
    elif rules:
        checked = _check_rules(rules)
        # 结构化规则只能在 Python 侧求值（见模块头注：beets 的范围查询
        # 对未注册类型的 flexible 字段会静默 0 命中）。所以这一支必须扫全库，
        # 但**只保留前 limit 条**——计数不占内存，内存峰值与库大小无关。
        page = []
        total = 0
        for item in lib.items():
            if all(_match(item, r) for r in checked):
                total += 1
                if len(page) < limit:
                    page.append(item)
    else:
        results = lib.items()
        total = len(results)
        page = list(itertools.islice(results, limit))

    return {
        "items": [_payload(it, want_fields) for it in page],
        "total": total,
        "truncated": total > len(page),
    }


def fields() -> dict:
    """字段清单：内置字段（带类型）+ 库里真出现过的 flexible 键 + 增强源三态。

    为什么不硬编码一份清单在宿主：flexible 键取决于用户装了哪些插件，
    写死在 UI 里就等于"插件装了也查不到"。
    """
    from beets.library import Item  # 局部 import：本模块其余命令不依赖 beets

    out = []
    for name, typ in Item._fields.items():
        out.append({
            "name": name,
            "type": _TYPE_NAMES.get(type(typ).__name__, "string"),
            "source": "builtin",
        })

    lib = open_library()
    # flexible 键只读 SQL：走 beets 的 API 要把全库 item 反序列化一遍，
    # 而这个命令只要"有哪些键"。只读连接，不会与写者抢锁。
    # URI 用正斜杠：Windows 下 `file:D:\x\y.db` 的反斜杠会被当转义。
    #
    # 失败**不吞**。曾经这里是 `except sqlite3.Error: flexible = []`，症状是
    # "插件明明装了、xtractor 也写出了 bpm，字段列表里却没有"——用户能看到的
    # 只有一个空列表，分不清是"没数据"还是"读不到"。宁可整条命令报错，也不要
    # 用空列表冒充一个答案。
    try:
        uri = f"file:{Path(str(lib.path)).as_posix()}?mode=ro"
        conn = sqlite3.connect(uri, uri=True, timeout=5)
        try:
            rows = conn.execute("SELECT DISTINCT key FROM item_attributes ORDER BY key")
            flexible = [r[0] for r in rows]
        finally:
            conn.close()
    except sqlite3.Error as e:
        raise ProtocolError(
            E_INVALID_ARG, _STAGE, f"读 flexible 字段失败（库不可读？）：{e}"
        ) from None
    for name in flexible:
        out.append({"name": name, "type": "flexible", "source": "flexible"})

    return {"fields": out, "sources": source_table()}
