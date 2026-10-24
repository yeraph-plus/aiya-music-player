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

PROTOCOL_VERSION = 2

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
        "plugins": plugins,
        "configDir": str(beets.config.config_dir()),
    }


# 递归扫描时收集的音频扩展名（与宿主扫描口径一致；v1 不含音轨提取类）
AUDIO_EXTENSIONS = {
    ".mp3", ".flac", ".m4a", ".aac", ".ogg", ".opus", ".wma", ".wav", ".aiff", ".alac",
}


def _open_library():
    """打开 beets 库（apply / dump 共用）。

    beets 2.14 实测坑：Library(path) 的 directory 默认取**系统 Music 目录**
    （platformdirs），不再读 config 的 directory；CLI 是显式传 config 值的
    （beets/ui/__init__.py:857），sidecar 照做，否则 move/copy 会写进用户的
    真实 Music 文件夹。
    """
    from beets import config as beets_config
    from beets.library import Library

    return Library(
        beets_config["library"].as_filename(),
        beets_config["directory"].as_filename(),
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
