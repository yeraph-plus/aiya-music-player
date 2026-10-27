"""曲库维护四件套：adopt / update / duplicates / move。

**共同铁律**：只碰 beets 库与文件，不碰播放器的库，也不发任何"已同步"事件——
对账是播放器的链路（本进程退出后 dump → 对齐）。本模块只负责让
「beets 库 + 磁盘」这一侧自洽。
"""

from __future__ import annotations

import os

from ..beetslib import (
    DIFF_FIELDS,
    configured_directory,
    dest_conflict,
    open_library,
    scan_audio_files,
)
from ..runtime import cancel_gen, emit


# ---------- 曲库维护（docs/29 批 D 四件套） ----------
#
#   adopt_library   自有库文件 → beets 库（as-is 入库：不联网 / 不改标签 / 不移动）
#   update_library  文件标签 → beets 库（beet update 语义，mtime 驱动）
#   find_duplicates beets 库内查重（duplicates 插件语义，只读）
#   move_items      beets 库按 path_formats 整理文件（beet move 语义，可预览）
#
# **共同铁律**：只碰 beets 库与文件，不碰宿主 DB，也不发 librarySynced ——
# 对账是宿主的链路（本进程退出后 dump_items → sync_items）。sidecar 只负责
# 让"beets 库 + 磁盘"这一侧自洽。
#
# 为什么全走**进程内 beets API** 而不是 spawn `python -m beets <cmd>`：
# CLI 的输出是给人看的文本，转成结构化事件既脆弱又拿不到逐曲粒度；而
# `Library` / `Item` 这套对象本来就够用（`beet update` 与 `beet move` 的核心
# 也是同一批调用）。代价是要自己复刻命令的**判定逻辑**——每处复刻都在
# 注释里标注了对应 beets 源码位置，改版本时照着比对。

# 维护任务的错误清单上限：几千个失败全塞进一个事件没有意义，UI 也只展示前几条
MAINT_MAX_ERRORS = 50
# 明细类事件（update 的字段变化、duplicates 的分组）的条数上限。
# 几千条 diff 会让一次会话的 stdout 膨胀到几十 MB，而用户根本看不过来；
# 超限只发计数，UI 显示"另有 N 条未列出"。
MAINT_MAX_DETAIL = 500
# 进度事件步长（文件数）：按文件发太吵（1846 首 = 1846 条事件），
# 按批次又会让进度条一顿一顿
MAINT_PROGRESS_STEP = 20

def norm(v) -> str:
    """标签值归一成可比较的字符串。

    为什么不能直接比：`item.get()` 对同一语义的字段会给出不同类型
    （`track` 可能是 int 也可能是 str，`length` 是 float，`path` 是 bytes），
    直接 `!=` 会把"没改"判成"改了"，update 的 diff 就全是噪声。
    """
    if v is None:
        return ""
    if isinstance(v, bytes):
        return v.decode("utf-8", "replace").strip()
    if isinstance(v, float):
        return f"{v:.3f}"
    return str(v).strip()


def item_brief(item) -> dict:
    """单条曲目的展示数据（查重与整理结果共用）。

    `size` 不是 Item 字段（2.14 实测 `getattr(item, "size", None)` 为 None），
    要现读磁盘；读不到就给 0 —— 文件已不在时照样要能显示这一行。
    """
    path = item.path.decode("utf-8", "replace")
    try:
        size = os.path.getsize(path)
    except OSError:
        size = 0
    return {
        "id": item.id,
        "path": path,
        "title": norm(item.get("title")),
        "artist": norm(item.get("artist")),
        "album": norm(item.get("album")),
        "length": round(float(item.get("length") or 0), 2),
        "bitrate": int(item.get("bitrate") or 0),
        "format": norm(item.get("format")),
        "size": size,
    }

def adopt_library(req_id, paths: list[str], gen: int) -> None:
    """把已有音频文件按现状登记进 beets 库。

    对应 beets 的 `import -A`（as-is）：只读标签入库，**不联网、不改标签、
    不移动文件**。移植的是 apply_import 里"入库"那一步，砍掉匹配与写盘。

    为什么需要它：整条链路只有 apply_import 会写 beets 库，用户用自有扫描器
    入库的曲目从没进过 beets，于是 update / duplicates / move 三条命令全部
    没有对象（真空库空转）。
    """
    session = f"adopt_{req_id:02d}"
    from beets.dbcore import query as dbcore_query
    from beets.library import Item

    lib = open_library()
    # 已在库的路径集合：一次全量读出比逐条 PathQuery 快一个量级
    # （1846 条逐条查 = 1846 次 SQL；本文件其它地方逐条查是为了拿整行，
    # 这里只要 path）
    known = {it.path.lower() for it in lib.items()}
    files = scan_audio_files([str(p) for p in paths])
    total = len(files)
    emit({"type": "progress", "session": session, "stage": "adopt",
           "current": 0, "total": total, "path": None})

    added = skipped = 0
    errors: list[dict] = []
    # 一个大事务：1846 条的逐条隐式提交会让 beets DB 做上千次 fsync。
    # 取消时是 `return`（正常退出 with）= 已登记的保留，不是回滚 ——
    # adopt 幂等，中断了重跑即可，用户不该丢掉已经等过的那几分钟。
    with lib.transaction():
        for i, p in enumerate(files):
            if cancel_gen() != gen:
                emit({"type": "cancelled", "session": session, "stage": "adopt"})
                return
            try:
                item = Item.from_path(p)
                if item.path.lower() in known:
                    skipped += 1
                else:
                    lib.add(item)
                    known.add(item.path.lower())
                    added += 1
            except Exception as e:  # noqa: BLE001 —— 单文件失败不中断整批
                if len(errors) < MAINT_MAX_ERRORS:
                    errors.append({"path": p, "message": f"{type(e).__name__}: {e}"})
            if (i + 1) % MAINT_PROGRESS_STEP == 0 or i + 1 == total:
                emit({"type": "progress", "session": session, "stage": "adopt",
                       "current": i + 1, "total": total, "path": p})
    emit({"type": "adoptDone", "session": session,
           "total": total, "added": added, "skipped": skipped,
           "failed": len(errors), "errors": errors})


# update 的比对与写回字段：DIFF_FIELDS（人看的标签）+ 文件事实字段。
# 少了后一组的话，文件被重新编码/换码率后 update 判"无变化"，落库的还是旧值。

UPDATE_FIELDS = DIFF_FIELDS + [
    "length", "bitrate", "samplerate", "bitdepth", "channels", "format",
]


def update_library(req_id, pretend: bool, gen: int) -> None:
    """用文件里的标签刷新 beets 库（beet update 语义）。

    三个判定照抄 `beets/ui/commands/update.py::update_items`（2.14.1）：
      1. 文件已不在 → 从 beets 库删行（缺失文件不该留着占位）
      2. `mtime` 没变 → 跳过，不重读标签（**这是 update 快的全部原因**）
      3. 逐字段比 diff，有变化才 `store(fields=...)`
    第 2 条尤其重要：不做 mtime 预筛的话，万首库每次 update 都要把每个文件
    重新读一遍标签，和"什么都没变"的用户预期完全相反。
    """
    from beets.util import syspath

    session = f"update_{req_id:02d}"
    lib = open_library()
    items = list(lib.items())
    total = len(items)
    emit({"type": "progress", "session": session, "stage": "update",
           "current": 0, "total": total, "path": None})

    changed = unchanged = missing = 0
    errors: list[dict] = []
    details: list[dict] = []
    for i, item in enumerate(items):
        if cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "update"})
            return
        try:
            path = item.path.decode("utf-8", "replace") if item.path else ""
            if not item.path or not os.path.exists(syspath(item.path)):
                missing += 1
                if not pretend:
                    item.remove(True)  # 文件已消失：删库行（磁盘上本来就没有）
            elif item.current_mtime() <= item.mtime:
                unchanged += 1
            else:
                before = {f: norm(item.get(f)) for f in UPDATE_FIELDS}
                item.read()  # 抛 ReadError 时进 except，落 failed
                diff = {}
                for f in UPDATE_FIELDS:
                    now = norm(item.get(f))
                    if before[f] != now:
                        diff[f] = {"from": before[f], "to": now}
                if diff:
                    changed += 1
                    if len(details) < MAINT_MAX_DETAIL:
                        details.append({"path": path, "changes": diff})
                    if not pretend:
                        item.store(fields=UPDATE_FIELDS)
                else:
                    unchanged += 1
                    if not pretend:
                        # 标签没变但 mtime 变了：把新 mtime 落库，否则下次
                        # update 又会重读这个文件（beets 同款处理）
                        item.store()
        except Exception as e:  # noqa: BLE001
            if len(errors) < MAINT_MAX_ERRORS:
                errors.append({
                    "path": item.path.decode("utf-8", "replace") if item.path else "",
                    "message": f"{type(e).__name__}: {e}",
                })
        if (i + 1) % MAINT_PROGRESS_STEP == 0 or i + 1 == total:
            emit({"type": "progress", "session": session, "stage": "update",
                   "current": i + 1, "total": total,
                   "path": item.path.decode("utf-8", "replace") if item.path else None})
    emit({"type": "updateDone", "session": session, "pretend": pretend,
           "total": total, "changed": changed, "unchanged": unchanged,
           "missing": missing, "failed": len(errors), "errors": errors,
           "details": details, "detailsTruncated": changed > len(details)})

def find_duplicates(req_id, keys: list[str], gen: int) -> None:
    """按指定字段组合查重（duplicates 插件语义，**只读**）。

    分组逻辑复刻 `beetsplug/duplicates.py::_group_by`（2.14.1）的非 strict
    分支：**所有 key 都为空才跳过**，部分为空仍参与分组（否则"只有标题、
    没有歌手"的一批曲目会被整批漏掉）。

    默认 key 为什么不是 beets 自带的 `mb_trackid/mb_albumid`：那两个字段只有
    联网匹配过（走 import 流程）才有值；as-is 入库的曲目全是 NULL，用默认
    key 查重必然零命中——看上去像"查重坏了"，实际是查重条件不适用。
    """
    session = f"dup_{req_id:02d}"
    keys = [k for k in (keys or []) if k] or ["title", "artist"]
    lib = open_library()
    items = list(lib.items())
    total = len(items)
    emit({"type": "progress", "session": session, "stage": "duplicates",
           "current": 0, "total": total, "path": None})

    groups: dict[tuple, list] = {}
    labels: dict[tuple, list[str]] = {}
    for i, item in enumerate(items):
        if cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "duplicates"})
            return
        vals = tuple(norm(item.get(k)).lower() for k in keys)
        if not any(vals):
            continue  # 全空：无从比较（与 _group_by 的 not strict 分支一致）
        if vals not in groups:
            # 分组 key 用小写归一值（大小写不同算重复），但**展示**要用首条
            # 的原始大小写——否则 UI 上会看到 "曲目a" 这种被压平的值，
            # 用户对着库里的 "曲目A" 找不到对应行
            labels[vals] = [norm(item.get(k)) for k in keys]
        groups.setdefault(vals, []).append(item)
        if (i + 1) % (MAINT_PROGRESS_STEP * 5) == 0 or i + 1 == total:
            emit({"type": "progress", "session": session, "stage": "duplicates",
                   "current": i + 1, "total": total, "path": None})

    dup_groups = [(k, objs) for k, objs in groups.items() if len(objs) > 1]
    # 重复条数多的组排前面：用户先要处理的永远是"同一首歌存了五份"
    dup_groups.sort(key=lambda kv: (-len(kv[1]), kv[0]))
    emitted = 0
    for k, objs in dup_groups:
        if emitted >= MAINT_MAX_DETAIL:
            break
        emitted += 1
        emit({"type": "duplicateGroup", "session": session,
               "key": labels[k], "count": len(objs),
               "items": [item_brief(it) for it in objs]})
    emit({"type": "duplicatesDone", "session": session,
           "keys": keys, "totalItems": total,
           "groups": len(dup_groups),
           "itemsInGroups": sum(len(o) for _k, o in dup_groups),
           "truncated": len(dup_groups) > emitted})

def move_items(req_id, directory: str, pretend: bool, gen: int) -> None:
    """按 path_formats 把 beets 库里的文件整理到 config 的 `directory` 下
    （beet move 语义）。

    **为什么目标根只能来自 config**：beets 2.14 已经把 items.path 存成
    **相对库根**的路径（DB 实测 `Non-Album/艺人/曲目.wav`），绝对路径靠
    `Library.directory` 现场拼。如果 move 用一个根、dump 用另一个根，
    宿主的 `beets_sync` 会拿错位的路径去对账（整库误判 orphan/imported）。
    所以基准只有一处：config 的 directory。

    `directory` 参数是**校验值**（宿主传它当前的库根）：不一致就拒绝执行，
    而不是猜一个。这比"默默用某个根"能解释得多。
    """
    from beets.util import MoveOperation

    from ..protocol import (
        E_LIBRARY_DIR_MISMATCH,
        E_NO_LIBRARY_DIR,
        ProtocolError,
    )

    session = f"move_{req_id:02d}"
    configured = configured_directory()
    # 三种拒绝都是**业务判定**，必须带自己的码回（以前抛 ValueError 被顶层
    # 兜底成 E_INTERNAL，宿主只能显示"工具内部异常"——而这三条恰恰是
    # 用户能自己解决的事：去配扫描目录、别在未知位置搬文件）。
    if not configured:
        raise ProtocolError(
            E_NO_LIBRARY_DIR, "move",
            "beets 配置里没有 directory（库根目录）：请先在「扫描入库」页设置扫描目录，"
            "或手改 beets 的 config.yaml",
        )
    want = (directory or "").strip()
    if want and os.path.normcase(os.path.abspath(want)) != os.path.normcase(
        os.path.abspath(configured)
    ):
        raise ProtocolError(
            E_LIBRARY_DIR_MISMATCH, "move",
            f"目标根不一致：应用配置 {want}，beets 配置 {configured}"
            "（拒绝在未知位置搬动文件）",
            requested=want, configured=configured,
        )
    if not os.path.isdir(configured):
        raise ProtocolError(
            E_NO_LIBRARY_DIR, "move", f"beets 库根目录不存在：{configured}",
            configured=configured,
        )

    lib = open_library(configured)
    items = list(lib.items())
    total = len(items)
    emit({"type": "progress", "session": session, "stage": "move",
           "current": 0, "total": total, "path": configured})

    planned = moved = same = conflicts = 0
    errors: list[dict] = []
    for i, item in enumerate(items):
        if cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "move"})
            return
        src = item.path.decode("utf-8", "replace") if item.path else ""
        try:
            dest = item.destination().decode("utf-8", "replace")
            # 目标 == 源（path_formats 生成的就是当前位置）→ 无需移动。
            # 用 normcase 比较：Windows 上盘符/分隔符大小写不同但指向同一文件
            if os.path.normcase(dest) == os.path.normcase(src):
                status, reason = "same", ""
            elif not src or not os.path.exists(src):
                status, reason = "conflict", "源文件不存在"
            else:
                conflict = dest_conflict(item, src)
                if conflict == "same":
                    status, reason = "same", ""
                elif conflict is not None:
                    status, reason = "conflict", conflict
                else:
                    status, reason = "move", ""
            if status == "same":
                same += 1
            elif status == "conflict":
                conflicts += 1
                if len(errors) < MAINT_MAX_ERRORS:
                    errors.append({"path": src, "message": reason})
            else:
                planned += 1
                if not pretend:
                    # beets 官方 move：建目录、搬文件、更新 item.path、清理空目录
                    item.move(MoveOperation.MOVE, with_album=False)
                    item.store()
                    moved += 1
            # 预览期每条都要发（用户要看"哪个文件去哪"），1846 条 ~400KB
            # 在管道上不算事；UI 用它渲染清单
            emit({"type": "moveItem", "session": session,
                   "kind": "plan" if pretend else "done",
                   "path": src, "dest": dest, "status": status, "reason": reason})
        except Exception as e:  # noqa: BLE001 —— 单曲失败不中断整批
            if len(errors) < MAINT_MAX_ERRORS:
                errors.append({"path": src, "message": f"{type(e).__name__}: {e}"})
        if (i + 1) % MAINT_PROGRESS_STEP == 0 or i + 1 == total:
            emit({"type": "progress", "session": session, "stage": "move",
                   "current": i + 1, "total": total, "path": src})
    emit({"type": "moveDone", "session": session, "pretend": pretend,
           "directory": configured, "total": total, "planned": planned,
           "moved": moved, "same": same, "conflict": conflicts,
           "failed": len(errors), "errors": errors})
