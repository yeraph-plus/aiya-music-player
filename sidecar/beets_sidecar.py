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
import sys
import traceback

PROTOCOL_VERSION = 1


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

# 当前标签 vs 候选：diff 展示字段（docs/08 §4.1）
DIFF_FIELDS = ["title", "artist", "album", "albumartist", "year", "track", "disc", "genre"]


def _emit(evt: dict) -> None:
    """流式事件（进度/专辑/结果）——与响应不同，无 id，靠 type 区分。"""
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


def preview_import(req_id, paths: list[str], options: dict) -> None:
    """只读预览：分组 → tag_album 候选 → album/progress 事件流。

    不写任何文件/库（B1 铁律）；匹配是网络操作（MusicBrainz），
    单组失败不中断整批（错误进事件，UI 可见）。
    """
    session = f"imp_{req_id:02d}"
    files = scan_audio_files(paths)
    groups = group_albums(files)
    total = len(groups)
    _emit({"type": "progress", "session": session, "stage": "scan",
           "current": 0, "total": total, "path": None})

    from beets import autotag

    for i, group_files in enumerate(groups):
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


def handle(req: dict) -> dict | None:
    """返回 None 表示已用事件流自行应答（preview_import 模式）。"""
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
        preview_import(req_id, [str(p) for p in paths], req.get("options") or {})
        return None
    return {"ok": False, "error": {"code": "E_UNKNOWN_CMD", "stage": "dispatch", "beets_message": f"unknown cmd: {cmd!r}"}}


def main() -> int:
    # beets 导入失败 = 没装 = status 也答不上来；回 not_found 让宿主给安装指引
    try:
        import beets  # noqa: F401
        import beets.ui  # noqa: F401
        _ = _status_payload  # 触发一次以提前暴露配置问题
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

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            resp = handle(req)
        except json.JSONDecodeError as e:
            resp = {"ok": False, "error": {"code": "E_BAD_JSON", "stage": "parse", "beets_message": str(e)}}
        except Exception as e:  # noqa: BLE001 —— sidecar 顶层兜底，任何异常都要回信封
            resp = {"ok": False, "error": {"code": "E_INTERNAL", "stage": "handle",
                                           "beets_message": f"{type(e).__name__}: {e}",
                                           "trace": traceback.format_exc(limit=4)}}
        try:
            req_id = req.get("id", 0) if isinstance(req, dict) else 0
        except Exception:  # noqa: BLE001
            req_id = 0
        # None = 命令已用事件流自行应答（preview_import），不再发 id 响应
        if resp is not None:
            sys.stdout.write(json.dumps({"id": req_id, **resp}, ensure_ascii=False) + "\n")
            sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
