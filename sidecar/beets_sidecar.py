#!/usr/bin/env python3
"""aiya-beets sidecar：stdio 行帧 JSON 协议（低频命令场景）。

协议（对齐 docs/08-beets.md §3.2）：
    请求  {"id": 1, "cmd": "status", ...}
    响应  {"id": 1, "ok": true, "data": ...} | {"id": 1, "ok": false, "error": {...}}

规则：
- 所有文件操作默认 no move / no copy / no write，直到 apply_import 显式下发
- 错误必须带 stage / path / beets_message（能定位到哪一步哪首歌）
- 不碰宿主 DB；只改 beets 库与文件标签，宿主通过事件刷新读模型

启动：python beets_sidecar.py   （宿主负责探测 python 并传 BEETSDIR 等环境）
"""

from __future__ import annotations

import json
import os
import platform
import queue
import sys
import threading
import traceback

PROTOCOL_VERSION = 3

# 取消代数（generation），不是布尔标志。
#
# 为什么不能用一个 Event：宿主发 cancel 时可能"请求行已进管道、主循环还没派发"
# —— 旧实现每次派发前 CANCEL.clear()，于是刚置的取消被下一条请求抹掉，取消
# 静默失效（表现为点了取消还在写标签）。改成自增代数后：
#   * 请求入队时记下当时的代数；派发时比较"当前代数是否变过"，
#     变过就说明自己在排队期间被取消过 → 直接跳过；
#   * 工作循环里同样比较代数，变过就收尾发 cancelled。
_CANCEL_GEN = 0
_CANCEL_LOCK = threading.Lock()


def _bump_cancel() -> int:
    global _CANCEL_GEN
    with _CANCEL_LOCK:
        _CANCEL_GEN += 1
        return _CANCEL_GEN


def _cancel_gen() -> int:
    with _CANCEL_LOCK:
        return _CANCEL_GEN


# stdout 写锁：reader 线程的 cancelAck 与主线程的事件流/响应并发写 stdout，
# 无锁时两行可能交错破坏行帧协议
_OUT_LOCK = threading.Lock()


def _force_utf8_stdio() -> None:
    """把 stdin/stdout 钉成 UTF-8（行协议的唯一编码）。

    为什么必须显式设置：Windows 上 Python 对管道用**本地编码**（简中 = cp936），
    `json.dumps(..., ensure_ascii=False)` 出来的中文会以 GBK 字节写出，宿主按
    UTF-8 读 → 非法字节 → 旧 reader 直接 break，整个会话无声死掉（用户看到的是
    "导入没反应"）。ensure_ascii=True 能绕过，但会把路径/标题全变 \\uXXXX，
    日志与断点调试都失去可读性，所以选择在协议层钉编码。
    """
    for stream, kwargs in (
        (sys.stdin, {"errors": "replace"}),
        (sys.stdout, {"errors": "replace", "newline": "\n"}),
    ):
        try:
            stream.reconfigure(encoding="utf-8", **kwargs)
        except (AttributeError, ValueError):  # 非标准流（测试替身/重定向）忽略
            pass


def _status_payload() -> dict:
    """beets 环境自检：版本/库/插件。beets 导入失败由调用方捕获成 not_found。"""
    import beets
    import beets.ui
    from beets import config as beets_config

    lib_path = beets_config["library"].as_filename()
    plugins = list(beets_config["plugins"].as_str_seq())
    lib_exists = os.path.exists(lib_path)
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "beetsVersion": beets.__version__,
        "pythonVersion": platform.python_version(),
        "libraryPath": str(lib_path),
        "libraryExists": lib_exists,
        # 库根目录：**只认 config.yaml 原文**（None = 没显式配过）。
        # 不用 config['directory'] 是因为它恒返回内置默认 ~/Music，
        # UI 拿它显示"整理到 C:\Users\x\Music"会直接误导用户（见
        # _configured_directory 的注释）。
        "libraryDirectory": _configured_directory(),
        "plugins": plugins,
        "configDir": str(beets.config.config_dir()),
    }


# 递归扫描时收集的音频扩展名（与宿主扫描口径一致；v1 不含音轨提取类）
AUDIO_EXTENSIONS = {
    ".mp3", ".flac", ".m4a", ".aac", ".ogg", ".opus", ".wma", ".wav", ".aiff", ".alac",
}


def _open_library(directory: str | None = None):
    """打开 beets 库（apply / dump / 维护共用）。

    beets 2.14 实测坑：Library(path) 的 directory 默认取**系统 Music 目录**
    （platformdirs），不再读 config 的 directory；CLI 是显式传 config 值的
    （beets/ui/__init__.py:857），sidecar 照做，否则 move/copy 会写进用户的
    真实 Music 文件夹。

    `directory` = 显式指定的目标根（批 D 的 move_items 用宿主传的扫描根）；
    None 时沿用 config 的 directory（`~/Music` 兜底）。
    """
    from beets import config as beets_config
    from beets.library import Library

    return Library(
        beets_config["library"].as_filename(),
        directory or beets_config["directory"].as_filename(),
    )

# 当前标签 vs 候选：diff 展示字段（docs/08 §4.1）
DIFF_FIELDS = ["title", "artist", "album", "albumartist", "year", "track", "disc", "genre"]


def _emit(evt: dict) -> None:
    """流式事件（进度/专辑/结果）——与响应不同，无 id，靠 type 区分。"""
    with _OUT_LOCK:
        sys.stdout.write(json.dumps(evt, ensure_ascii=False) + "\n")
        sys.stdout.flush()


def scan_audio_files(paths: list[str]) -> list[str]:
    """展开目录为音频文件列表（递归）；直接给文件则原样收下。"""
    out: list[str] = []
    for p in paths:
        if os.path.isfile(p):
            out.append(p)
        elif os.path.isdir(p):
            for root, _dirs, files in os.walk(p):
                for name in sorted(files):
                    if os.path.splitext(name)[1].lower() in AUDIO_EXTENSIONS:
                        out.append(os.path.join(root, name))
    return out


def group_albums(files: list[str]) -> list[list[str]]:
    """v1 分组：同目录 = 同专辑（含子目录里落单的散曲各自成组）。

    不读标签做簇分组（那是 beets importer 的活，v1 不复刻）；
    同目录混多专辑时用户可以在 UI 里逐曲处理（singleton 后续补）。
    """
    by_dir: dict[str, list[str]] = {}
    for f in files:
        by_dir.setdefault(os.path.dirname(f), []).append(f)
    return [files_in_dir for _d, files_in_dir in sorted(by_dir.items())]


def _item_meta(item) -> dict:
    """Item 当前标签 → UI diff 展示用（DIFF_FIELDS + 文件信息）。"""
    meta = {}
    for f in DIFF_FIELDS:
        v = item.get(f)
        if isinstance(v, bytes):
            v = v.decode("utf-8", "replace")
        meta[f] = v
    meta["path"] = str(item.path.decode("utf-8", "replace"))
    meta["length"] = round(float(item.length or 0), 2)
    return meta


def _track_info_meta(info) -> dict:
    """TrackInfo（候选侧单曲）→ 与 _item_meta 同形，UI 才能逐字段对齐。"""
    meta = {}
    for f in DIFF_FIELDS:
        v = info.get(f) if hasattr(info, "get") else getattr(info, f, None)
        if isinstance(v, bytes):
            v = v.decode("utf-8", "replace")
        meta[f] = v
    meta["length"] = round(float(getattr(info, "length", 0) or 0), 2)
    return meta


def _album_meta(info) -> dict:
    """AlbumInfo（候选侧专辑级）→ 展示字段。"""
    return {
        "id": info.get("album_id") if hasattr(info, "get") else None,
        "album": info.get("album") if hasattr(info, "get") else None,
        "artist": info.get("artist") if hasattr(info, "get") else None,
        "year": info.get("year") if hasattr(info, "get") else None,
        "genre": info.get("genre") if hasattr(info, "get") else None,
        "dataSource": info.get("data_source") if hasattr(info, "get") else None,
        "tracks": [_track_info_meta(t) for t in (info.tracks or [])],
    }


def _candidate_payload(cid: str, match) -> dict:
    """AlbumMatch → UI 候选卡片（置信度 + 推荐 + 逐曲对齐 diff）。"""
    distance = getattr(match.distance, "distance", 1.0)
    pairs = match.item_info_pairs
    return {
        "id": cid,
        "confidence": round(max(0.0, min(1.0, 1.0 - distance)), 3),
        "fromScratch": bool(getattr(match, "from_scratch", False)),
        "extraItems": len(getattr(match, "extra_items", []) or []),
        "extraTracks": len(getattr(match, "extra_tracks", []) or []),
        "album": _album_meta(match.info),
        # 逐曲对齐：item.path → 候选 TrackInfo（对不上的在 extra 里）
        "mapping": {
            item.path.decode("utf-8", "replace"): _track_info_meta(info)
            for item, info in pairs
        },
    }


def preview_import(req_id, paths: list[str], options: dict, gen: int) -> None:
    """只读预览：分组 → tag_album 候选 → album/progress 事件流。

    不写任何文件/库（B1 铁律）；匹配是网络操作（MusicBrainz），
    单组失败不中断整批（错误进事件，UI 可见）。

    `gen` = 请求入队时的取消代数（见 _bump_cancel 注释）。
    """
    session = f"imp_{req_id:02d}"
    files = scan_audio_files(paths)
    groups = group_albums(files)
    total = len(groups)
    _emit({"type": "progress", "session": session, "stage": "scan",
           "current": 0, "total": total, "path": None})

    from beets import autotag

    for i, group_files in enumerate(groups):
        if _cancel_gen() != gen:
            _emit({"type": "cancelled", "session": session, "stage": "matching"})
            return
        first = group_files[0]
        _emit({"type": "progress", "session": session, "stage": "matching",
               "current": i + 1, "total": total, "path": os.path.dirname(first) or first})
        try:
            from beets.library import Item
            items = [Item.from_path(f) for f in group_files]
            source = autotag.Source.from_items(items)
            proposal = autotag.tag_album(source)
            # 2.14 实测 Proposal.candidates 是 list[AlbumMatch]（源码注释称 dict 已过时）
            raw = proposal.candidates
            if hasattr(raw, "values"):
                raw = list(raw.values())
            candidates = [
                _candidate_payload(f"cand_{i:03d}_{j:02d}", m)
                for j, m in enumerate(raw)
            ]
            _emit({
                "type": "album", "session": session,
                "album": {
                    "id": f"alb_{i:03d}",
                    "paths": group_files,
                    "recommendation": proposal.recommendation.name,
                    "current": {
                        "album": source.name,
                        "artist": source.artist,
                        "tracks": [_item_meta(it) for it in items],
                    },
                    "candidates": candidates,
                },
            })
        except Exception as e:  # noqa: BLE001 —— 单组失败不中断整批
            _emit({"type": "albumError", "session": session, "stage": "matching",
                   "path": first, "beets_message": f"{type(e).__name__}: {e}"})
    _emit({"type": "done", "session": session, "albums": total})


# 应用标签的字段（与 DIFF_FIELDS 同源：预览看到什么，应用就写什么）
APPLY_FIELDS = DIFF_FIELDS

# action → 文件操作（docs/08 §4.1：默认只写标签，移动/复制显式选择）。
# move/copy 用 beets 自己的 Item.move：按用户 path_formats 算目标路径、
# 建目录、更新 item.path、MOVE 后清理空目录。
FILE_ACTIONS = {}


def _load_file_actions():
    from beets.util import MoveOperation

    FILE_ACTIONS.update({
        "apply_move": MoveOperation.MOVE,
        "apply_copy": MoveOperation.COPY,
    })


def _dest_conflict(item, src: str) -> str | None:
    """计算 item 的 beets 目标路径并检查冲突。

    beets 的 move_file 遇到目标已存在会**静默改名**（unique_path → name.1.ext），
    不符合"目标冲突先报错"；所以这里在 move 之前自查，冲突就跳过文件操作。
    返回冲突描述（None = 无冲突；"same" = 目标就是源文件本身，move 是 no-op）。
    """
    dest = item.destination()  # bytes，按 lib.path_formats + db.directory
    if not os.path.exists(dest):
        return None
    try:
        if os.path.samefile(dest, src):
            return "same"  # 库目录就是来源目录：无需移动
    except OSError:
        pass
    return f"目标已存在：{dest.decode('utf-8', 'replace')}"


def apply_import(req_id, decisions: list[dict], gen: int) -> None:
    """应用导入决策：写标签到文件 + 入 beets 库（默认不移动文件）。

    decisions: [{"album": "alb_01", "action": "apply" | "apply_move" |
                 "apply_copy" | "skip",
                 "paths": [...],
                 "candidate": {"album": {...专辑级...}, "mapping": {path: 单曲meta}}}]
    候选数据由 UI 从预览结果原样回传 —— 不重跑网络匹配，保证"所见即所写"；
    这也是为什么 apply 进程不需要与 preview 进程共享 session 状态（进程是一次性的）。

    move/copy 语义：写标签 + 入库后，目标路径冲突的曲目**不覆盖不改名**，
    报 conflict 失败（文件留在原地、标签已写，解决冲突后重新应用即可完成移动）。

    进度**按文件**发（不是按专辑）：宿主有 10 分钟无输出就判卡死的看门狗，
    一张几百首的专辑或慢速磁盘上"专辑级"粒度太粗，会被误杀。
    """
    session = f"app_{req_id:02d}"
    from beets.dbcore import query as dbcore_query
    from beets.library import Item

    _load_file_actions()
    lib = _open_library()

    active = [d for d in decisions if d.get("action") in ("apply", "apply_move", "apply_copy")]
    apply_total = len(active)
    file_total = sum(len(d.get("paths") or []) for d in active)
    file_done = 0
    _emit({"type": "progress", "session": session, "stage": "writing",
           "current": 0, "total": apply_total, "path": None})

    applied = skipped = failed_files = moved_files = 0
    for i, dec in enumerate(decisions):
        action = dec.get("action")
        op = FILE_ACTIONS.get(action)
        if action not in ("apply", "apply_move", "apply_copy"):
            skipped += 1
            continue
        paths = [str(p) for p in (dec.get("paths") or [])]
        _emit({"type": "progress", "session": session, "stage": "writing",
               "current": i + 1, "total": apply_total,
               "path": paths[0] if paths else None})
        cand = dec.get("candidate") or {}
        album_meta = cand.get("album") or {}
        mapping = cand.get("mapping") or {}
        written, failures = 0, []
        for p in paths:
            if _cancel_gen() != gen:
                _emit({"type": "cancelled", "session": session, "stage": "writing"})
                return
            # 逐文件进度：既是 UI 的真实进度，也是看门狗的心跳
            file_done += 1
            _emit({"type": "progress", "session": session, "stage": "writing",
                   "current": i + 1, "total": apply_total,
                   "fileCurrent": file_done, "fileTotal": file_total, "path": p})
            try:
                item = Item.from_path(p)
                # 专辑级字段打底，单曲级非 None 值覆盖（对齐 beets 的 merge 语义）
                data = {}
                for f in APPLY_FIELDS:
                    val = album_meta.get(f)
                    track_meta = mapping.get(p) or {}
                    if track_meta.get(f) is not None:
                        val = track_meta[f]
                    if val is not None:
                        data[f] = val
                item.update(data)
                item.write()  # 写标签进文件（可抛 ReadError/WriteError）
                # 入库（幂等）：同路径已在 beets 库 → 更新行；否则新增
                dup = list(lib.items(dbcore_query.PathQuery("path", item.path)))
                if dup:
                    for e in dup:
                        e.update(data)
                        e.store()
                    mover = dup[0] if op else None
                else:
                    lib.add(item)
                    mover = item if op else None
                written += 1
                if mover is not None:
                    conflict = _dest_conflict(mover, p)
                    if conflict == "same":
                        pass  # 目标 == 源：无需移动
                    elif conflict is not None:
                        # 不覆盖不改名：文件留在原地（标签已写），可解决后重试
                        failures.append({"path": p, "code": "conflict", "message": conflict})
                    else:
                        # beets 官方 move：建目录、搬文件、更新 item.path、清理空目录
                        mover.move(op, with_album=False)
                        moved_files += 1
            except Exception as e:  # noqa: BLE001 —— 单曲失败不中断整张
                failures.append({"path": p, "message": f"{type(e).__name__}: {e}"})
        failed_files += len(failures)
        applied += 1
        _emit({"type": "applied", "session": session, "albumId": dec.get("album"),
               "written": written, "moved": op is not None, "failed": failures})
    _emit({"type": "applyDone", "session": session,
           "applied": applied, "skipped": skipped, "failed": failed_files,
           "moved": moved_files})


# dump_items 输出的字段白名单：只给宿主读模型对齐需要的（beets 字段名原样，
# 宿主侧映射到自有列）。path 是对账 key。
DUMP_FIELDS = [
    "title", "artist", "albumartist", "album", "genre",
    "year", "track", "disc", "length", "bitrate", "format",
    "samplerate", "bitdepth", "channels", "mtime", "rg_track_gain",
]


def dump_items(req_id) -> None:
    """流式导出 beets 库 items（宿主读模型对齐用，docs/08 §5 读模型同步）。

    每行一个 item 事件（几万条也不膨胀内存）；最后 dumpDone 带总数。
    """
    session = f"sync_{req_id:02d}"
    lib = _open_library()
    total = 0
    for item in lib.items():
        payload = {"id": item.id, "path": item.path.decode("utf-8", "replace")}
        for f in DUMP_FIELDS:
            v = item.get(f)
            if isinstance(v, bytes):
                v = v.decode("utf-8", "replace")
            elif f == "mtime" and v is not None:
                # beets 的 DATE 类型读出来是 float（1792771928.0），契约是整型秒
                v = int(v)
            payload[f] = v
        total += 1
        _emit({"type": "item", "session": session, "item": payload})
    _emit({"type": "dumpDone", "session": session, "items": total})


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


def _norm(v) -> str:
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


def _item_brief(item) -> dict:
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
        "title": _norm(item.get("title")),
        "artist": _norm(item.get("artist")),
        "album": _norm(item.get("album")),
        "length": round(float(item.get("length") or 0), 2),
        "bitrate": int(item.get("bitrate") or 0),
        "format": _norm(item.get("format")),
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

    lib = _open_library()
    # 已在库的路径集合：一次全量读出比逐条 PathQuery 快一个量级
    # （1846 条逐条查 = 1846 次 SQL；本文件其它地方逐条查是为了拿整行，
    # 这里只要 path）
    known = {it.path.lower() for it in lib.items()}
    files = scan_audio_files([str(p) for p in paths])
    total = len(files)
    _emit({"type": "progress", "session": session, "stage": "adopt",
           "current": 0, "total": total, "path": None})

    added = skipped = 0
    errors: list[dict] = []
    # 一个大事务：1846 条的逐条隐式提交会让 beets DB 做上千次 fsync。
    # 取消时是 `return`（正常退出 with）= 已登记的保留，不是回滚 ——
    # adopt 幂等，中断了重跑即可，用户不该丢掉已经等过的那几分钟。
    with lib.transaction():
        for i, p in enumerate(files):
            if _cancel_gen() != gen:
                _emit({"type": "cancelled", "session": session, "stage": "adopt"})
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
                _emit({"type": "progress", "session": session, "stage": "adopt",
                       "current": i + 1, "total": total, "path": p})
    _emit({"type": "adoptDone", "session": session,
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
    lib = _open_library()
    items = list(lib.items())
    total = len(items)
    _emit({"type": "progress", "session": session, "stage": "update",
           "current": 0, "total": total, "path": None})

    changed = unchanged = missing = 0
    errors: list[dict] = []
    details: list[dict] = []
    for i, item in enumerate(items):
        if _cancel_gen() != gen:
            _emit({"type": "cancelled", "session": session, "stage": "update"})
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
                before = {f: _norm(item.get(f)) for f in UPDATE_FIELDS}
                item.read()  # 抛 ReadError 时进 except，落 failed
                diff = {}
                for f in UPDATE_FIELDS:
                    now = _norm(item.get(f))
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
            _emit({"type": "progress", "session": session, "stage": "update",
                   "current": i + 1, "total": total,
                   "path": item.path.decode("utf-8", "replace") if item.path else None})
    _emit({"type": "updateDone", "session": session, "pretend": pretend,
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
    lib = _open_library()
    items = list(lib.items())
    total = len(items)
    _emit({"type": "progress", "session": session, "stage": "duplicates",
           "current": 0, "total": total, "path": None})

    groups: dict[tuple, list] = {}
    labels: dict[tuple, list[str]] = {}
    for i, item in enumerate(items):
        if _cancel_gen() != gen:
            _emit({"type": "cancelled", "session": session, "stage": "duplicates"})
            return
        vals = tuple(_norm(item.get(k)).lower() for k in keys)
        if not any(vals):
            continue  # 全空：无从比较（与 _group_by 的 not strict 分支一致）
        if vals not in groups:
            # 分组 key 用小写归一值（大小写不同算重复），但**展示**要用首条
            # 的原始大小写——否则 UI 上会看到 "曲目a" 这种被压平的值，
            # 用户对着库里的 "曲目A" 找不到对应行
            labels[vals] = [_norm(item.get(k)) for k in keys]
        groups.setdefault(vals, []).append(item)
        if (i + 1) % (MAINT_PROGRESS_STEP * 5) == 0 or i + 1 == total:
            _emit({"type": "progress", "session": session, "stage": "duplicates",
                   "current": i + 1, "total": total, "path": None})

    dup_groups = [(k, objs) for k, objs in groups.items() if len(objs) > 1]
    # 重复条数多的组排前面：用户先要处理的永远是"同一首歌存了五份"
    dup_groups.sort(key=lambda kv: (-len(kv[1]), kv[0]))
    emitted = 0
    for k, objs in dup_groups:
        if emitted >= MAINT_MAX_DETAIL:
            break
        emitted += 1
        _emit({"type": "duplicateGroup", "session": session,
               "key": labels[k], "count": len(objs),
               "items": [_item_brief(it) for it in objs]})
    _emit({"type": "duplicatesDone", "session": session,
           "keys": keys, "totalItems": total,
           "groups": len(dup_groups),
           "itemsInGroups": sum(len(o) for _k, o in dup_groups),
           "truncated": len(dup_groups) > emitted})


def _configured_directory() -> str | None:
    """从 config.yaml **原文**读 `directory`，没显式配置就返回 None。

    为什么不能问 beets：`config['directory'].get()` 恒返回内置默认值
    （2.14 实测：config.yaml 不存在的库也返回 `~/Music`，来自
    `config_default.yaml`），于是"用户配过"和"用的是默认"完全无法区分。
    拿这个默认值去 move 等于把用户的曲库搬进他真实的音乐文件夹。

    只认行首无缩进的 `directory:`（缩进的属于别的 section）。
    返回值做了 `expanduser`：手写 `directory: ~/Music` 是合法 YAML，而
    `Library(path, "~/Music")` 不会展开 `~`（beets 的 `as_filename()` 才会），
    不展开就会拼出 `<cwd>/~/Music` 这种路径去搬文件。
    """
    from beets import config as beets_config

    cfg = os.path.join(str(beets_config.config_dir()), "config.yaml")
    try:
        with open(cfg, encoding="utf-8") as f:
            for line in f:
                if line[:1] in (" ", "\t", "#"):
                    continue
                if line.startswith("directory:"):
                    val = line[len("directory:"):].strip().strip("'\"")
                    return os.path.expanduser(val) if val else None
    except OSError:
        return None
    return None


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

    session = f"move_{req_id:02d}"
    configured = _configured_directory()
    if not configured:
        raise ValueError(
            "beets 配置里没有 directory（库根目录）：请先在「扫描入库」页设置扫描目录，"
            "或手改 beets 的 config.yaml"
        )
    want = (directory or "").strip()
    if want and os.path.normcase(os.path.abspath(want)) != os.path.normcase(
        os.path.abspath(configured)
    ):
        raise ValueError(
            f"目标根不一致：应用配置 {want}，beets 配置 {configured}（拒绝在未知位置搬动文件）"
        )
    if not os.path.isdir(configured):
        raise ValueError(f"beets 库根目录不存在：{configured}")

    lib = _open_library(configured)
    items = list(lib.items())
    total = len(items)
    _emit({"type": "progress", "session": session, "stage": "move",
           "current": 0, "total": total, "path": configured})

    planned = moved = same = conflicts = 0
    errors: list[dict] = []
    for i, item in enumerate(items):
        if _cancel_gen() != gen:
            _emit({"type": "cancelled", "session": session, "stage": "move"})
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
                conflict = _dest_conflict(item, src)
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
            _emit({"type": "moveItem", "session": session,
                   "kind": "plan" if pretend else "done",
                   "path": src, "dest": dest, "status": status, "reason": reason})
        except Exception as e:  # noqa: BLE001 —— 单曲失败不中断整批
            if len(errors) < MAINT_MAX_ERRORS:
                errors.append({"path": src, "message": f"{type(e).__name__}: {e}"})
        if (i + 1) % MAINT_PROGRESS_STEP == 0 or i + 1 == total:
            _emit({"type": "progress", "session": session, "stage": "move",
                   "current": i + 1, "total": total, "path": src})
    _emit({"type": "moveDone", "session": session, "pretend": pretend,
           "directory": configured, "total": total, "planned": planned,
           "moved": moved, "same": same, "conflict": conflicts,
           "failed": len(errors), "errors": errors})


def handle(req: dict, gen: int) -> dict | None:
    """返回 None 表示已用事件流自行应答（preview/apply/dump 模式）。

    `gen` = 该请求入队时的取消代数：主循环据此判断"排队期间是否被取消过"。
    """
    cmd = req.get("cmd")
    if cmd == "status":
        return {"ok": True, "data": _status_payload()}
    if cmd == "ping":
        return {"ok": True, "data": {"protocolVersion": PROTOCOL_VERSION}}
    if cmd == "preview_import":
        req_id = int(req.get("id") or 0)
        paths = req.get("paths") or []
        if not paths:
            return {"ok": False, "error": {"code": "E_BAD_ARGS", "stage": "preview",
                                           "beets_message": "paths 为空"}}
        preview_import(req_id, [str(p) for p in paths], req.get("options") or {}, gen)
        return None
    if cmd == "apply_import":
        req_id = int(req.get("id") or 0)
        decisions = req.get("decisions") or []
        if not decisions:
            return {"ok": False, "error": {"code": "E_BAD_ARGS", "stage": "apply",
                                           "beets_message": "decisions 为空"}}
        apply_import(req_id, decisions, gen)
        return None
    if cmd == "dump_items":
        dump_items(int(req.get("id") or 0))
        return None
    # ---- 曲库维护（批 D）----
    if cmd == "adopt_library":
        paths = req.get("paths") or []
        if not paths:
            return {"ok": False, "error": {"code": "E_BAD_ARGS", "stage": "adopt",
                                           "beets_message": "paths 为空"}}
        adopt_library(int(req.get("id") or 0), [str(p) for p in paths], gen)
        return None
    if cmd == "update_library":
        update_library(int(req.get("id") or 0), bool(req.get("pretend")), gen)
        return None
    if cmd == "find_duplicates":
        find_duplicates(int(req.get("id") or 0), req.get("keys") or [], gen)
        return None
    if cmd == "move_items":
        move_items(int(req.get("id") or 0), str(req.get("directory") or ""),
                   bool(req.get("pretend")), gen)
        return None
    return {"ok": False, "error": {"code": "E_UNKNOWN_CMD", "stage": "dispatch", "beets_message": f"unknown cmd: {cmd!r}"}}


def main() -> int:
    # 编码先钉死，再考虑输出任何一行（含下面的 not_found 信封）
    _force_utf8_stdio()
    # beets 导入失败 = 没装 = status 也答不上来；回 not_found 让宿主给安装指引
    try:
        import beets  # noqa: F401
        import beets.ui  # noqa: F401
        try:
            _status_payload()  # 真调用一次：提前暴露 config 问题（此前只取了函数引用，没调用）
        except Exception:  # noqa: BLE001 —— config 错误不打断启动，status 请求时会给出具体错误信封
            pass
    except ImportError:
        sys.stdout.write(json.dumps({
            "id": 0, "ok": False,
            "error": {"code": "E_BEETS_NOT_FOUND", "stage": "import",
                      "beets_message": "beets is not installed for this interpreter"},
        }) + "\n")
        sys.stdout.flush()
        return 0
    except SystemExit:
        # beets.ui 导入可能触发 argparse SystemExit；吞掉，按导入失败处理
        sys.stdout.write(json.dumps({
            "id": 0, "ok": False,
            "error": {"code": "E_BEETS_NOT_FOUND", "stage": "import",
                      "beets_message": "beets import failed (SystemExit)"},
        }) + "\n")
        sys.stdout.flush()
        return 0

    # cancel 必须在工作循环运行时也能被接收：后台线程持续读 stdin。
    # cancel 在 reader 线程**立即递增取消代数**——此前 cancel 行与其他请求
    # 一样进队列，而主循环正同步执行 preview/apply（网络匹配+写文件），
    # req_q.get() 根本轮不到它，取消永远看不到。
    # 队列元素是 (行, 入队时的取消代数)：排队期间被取消过的请求在派发时
    # 就能识别出来（旧实现的 CANCEL.clear() 会把这个取消抹掉）。
    req_q: queue.Queue = queue.Queue()

    def _read_stdin() -> None:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                req = json.loads(line)
            except json.JSONDecodeError:
                req = None
            if isinstance(req, dict) and req.get("cmd") == "cancel":
                _bump_cancel()
                _emit({"type": "cancelAck"})
                continue
            req_q.put((line, _cancel_gen()))
        req_q.put(None)  # EOF

    threading.Thread(target=_read_stdin, name="stdin-reader", daemon=True).start()

    while True:
        item = req_q.get()
        if item is None:
            break
        line, gen = item
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            resp = {"ok": False, "error": {"code": "E_BAD_JSON", "stage": "parse", "beets_message": str(e)}}
            req_id = 0
        else:
            if isinstance(req, dict) and req.get("cmd") == "cancel":
                # 兜底分支（正常路径已在 reader 线程旁路处理）
                _bump_cancel()
                _emit({"type": "cancelAck"})
                continue
            try:
                resp = handle(req, gen)
            except Exception as e:  # noqa: BLE001 —— sidecar 顶层兜底，任何异常都要回信封
                resp = {"ok": False, "error": {"code": "E_INTERNAL", "stage": "handle",
                                               "beets_message": f"{type(e).__name__}: {e}",
                                               "trace": traceback.format_exc(limit=4)}}
            try:
                req_id = req.get("id", 0) if isinstance(req, dict) else 0
            except Exception:  # noqa: BLE001
                req_id = 0
        # None = 命令已用事件流自行应答（preview_import 等），不再发 id 响应
        if resp is not None:
            payload = {"id": req_id, **resp}
            if payload.get("ok") is False:
                # 会话模式：错误信封也是终止事件（P2：宿主 reader 只认带 type
                # 的终止事件，错误信封曾让会话卡死在等 EOF、槽位占死）
                payload["type"] = "sessionFailed"
            with _OUT_LOCK:
                sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
                sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
