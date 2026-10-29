"""导入：预览（只读网络匹配）与应用（写标签 + 入库，可移动/复制）。

铁律：预览不写任何文件与库；应用只在 decisions 显式下发后才动文件。
"""

from __future__ import annotations

import os
import re

from .. import checkpoints, evidence
from ..beetslib import DIFF_FIELDS, dest_conflict, group_albums, open_library, scan_audio_files
from ..protocol import E_INVALID_ARG, ProtocolError
from ..runtime import cancel_gen, emit

# searchIds 的格式门槛（v2）：小写、带连字符的 MusicBrainz UUID。
# 大写/无连字符一律拒——MB 的 API 认这个形状，宽松放行只会把错误传给上游。
_MB_RELEASE_ID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")


def item_meta(item) -> dict:
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


def track_info_meta(info) -> dict:
    """TrackInfo（候选侧单曲）→ 与 _item_meta 同形，UI 才能逐字段对齐。"""
    meta = {}
    for f in DIFF_FIELDS:
        v = info.get(f) if hasattr(info, "get") else getattr(info, f, None)
        if isinstance(v, bytes):
            v = v.decode("utf-8", "replace")
        meta[f] = v
    meta["length"] = round(float(getattr(info, "length", 0) or 0), 2)
    return meta


def album_meta(info) -> dict:
    """AlbumInfo（候选侧专辑级）→ 展示字段。"""
    return {
        "id": info.get("album_id") if hasattr(info, "get") else None,
        "album": info.get("album") if hasattr(info, "get") else None,
        "artist": info.get("artist") if hasattr(info, "get") else None,
        "year": info.get("year") if hasattr(info, "get") else None,
        "genre": info.get("genre") if hasattr(info, "get") else None,
        "dataSource": info.get("data_source") if hasattr(info, "get") else None,
        "tracks": [track_info_meta(t) for t in (info.tracks or [])],
    }


def candidate_payload(cid: str, match) -> dict:
    """AlbumMatch → UI 候选卡片（置信度 + 推荐 + 证据分层 + 逐曲对齐 diff）。"""
    distance = getattr(match.distance, "distance", 1.0)
    pairs = match.item_info_pairs
    tier, reasons = evidence.compute(match)
    return {
        "id": cid,
        "confidence": round(max(0.0, min(1.0, 1.0 - distance)), 3),
        "fromScratch": bool(getattr(match, "from_scratch", False)),
        "extraItems": len(getattr(match, "extra_items", []) or []),
        "extraTracks": len(getattr(match, "extra_tracks", []) or []),
        # v2：证据分层 + 人话依据（UI 显示"为什么推荐这个"；机器判定看 tier）
        "evidence": tier,
        "reasons": reasons,
        "album": album_meta(match.info),
        # 逐曲对齐：item.path → 候选 TrackInfo（对不上的在 extra 里）
        "mapping": {
            item.path.decode("utf-8", "replace"): track_info_meta(info)
            for item, info in pairs
        },
    }

def preview_import(req_id, paths: list[str], options: dict, gen: int) -> None:
    """只读预览：分组 → tag_album 候选 → album/progress 事件流。

    不写任何文件/库（B1 铁律）；匹配是网络操作（MusicBrainz），
    单组失败不中断整批（错误进事件，UI 可见）。

    options.excludePaths：已导入排除表（docs/40 十三轮）。待整理目录固定后，
    copy 应用会把原件留在 inbox——不排除的话下次整理会把同一批文件再导一遍
    （beets 库重复条目 → dump 同步 → 曲库出现离线重复行）。宿主记录成功
    应用的源路径并在这里过滤。大小写不敏感（Windows 路径）。

    options.searchIds（v2）：用户钉死的 MusicBrainz Release ID 列表。给出时
    匹配限定在这些 release（"手动重刮"：autotag 认不准的专辑让用户直接指认）。
    beets 的 tag_album 原生支持（`search_ids` 参数）；对预览里的**每个**专辑组
    都尝试，所以通常配合单个文件夹使用。校验放在任何事件发出之前——非法
    参数整批拒绝，不出一半结果再炸。

    `gen` = 请求入队时的取消代数（见 bump_cancel 注释）。
    """
    session = f"imp_{req_id:02d}"
    search_ids = [s for s in ((options or {}).get("searchIds") or []) if isinstance(s, str) and s.strip()]
    bad = [s for s in search_ids if not _MB_RELEASE_ID.match(s.strip())]
    if bad:
        raise ProtocolError(
            E_INVALID_ARG, "preview",
            f"searchIds 必须是小写 MusicBrainz Release ID（UUID 带连字符）：{bad[:3]}",
            invalid=bad[:3],
        )
    search_ids = [s.strip() for s in search_ids]
    files = scan_audio_files(paths)
    exclude = {
        os.path.normcase(os.path.abspath(p))
        for p in (options or {}).get("excludePaths") or []
        if isinstance(p, str) and p.strip()
    }
    if exclude:
        files = [f for f in files if os.path.normcase(os.path.abspath(f)) not in exclude]
    groups = group_albums(files)
    total = len(groups)
    emit({"type": "progress", "session": session, "stage": "scan",
           "current": 0, "total": total, "path": None})

    from beets import autotag

    for i, group_files in enumerate(groups):
        if cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "matching"})
            return
        first = group_files[0]
        emit({"type": "progress", "session": session, "stage": "matching",
               "current": i + 1, "total": total, "path": os.path.dirname(first) or first})
        try:
            from beets.library import Item
            items = [Item.from_path(f) for f in group_files]
            source = autotag.Source.from_items(items)
            proposal = autotag.tag_album(source, search_ids=search_ids or None)
            # 2.14 实测 Proposal.candidates 是 list[AlbumMatch]（源码注释称 dict 已过时）
            raw = proposal.candidates
            if hasattr(raw, "values"):
                raw = list(raw.values())
            candidates = [
                candidate_payload(f"cand_{i:03d}_{j:02d}", m)
                for j, m in enumerate(raw)
            ]
            emit({
                "type": "album", "session": session,
                "album": {
                    "id": f"alb_{i:03d}",
                    "paths": group_files,
                    "recommendation": proposal.recommendation.name,
                    "current": {
                        "album": source.name,
                        "artist": source.artist,
                        "tracks": [item_meta(it) for it in items],
                    },
                    "candidates": candidates,
                },
            })
        except Exception as e:  # noqa: BLE001 —— 单组失败不中断整批
            emit({"type": "albumError", "session": session, "stage": "matching",
                   "path": first, "beets_message": f"{type(e).__name__}: {e}"})
    emit({"type": "done", "session": session, "albums": total})

APPLY_FIELDS = DIFF_FIELDS

# action → 文件操作（docs/08 §4.1：默认只写标签，移动/复制显式选择）。
# move/copy 用 beets 自己的 Item.move：按用户 path_formats 算目标路径、
# 建目录、更新 item.path、MOVE 后清理空目录。
FILE_ACTIONS = {}


def load_file_actions():
    from beets.util import MoveOperation

    FILE_ACTIONS.update({
        "apply_move": MoveOperation.MOVE,
        "apply_copy": MoveOperation.COPY,
    })


def apply_import(req_id, decisions: list[dict], resume: bool, gen: int) -> None:
    """应用导入决策：写标签到文件 + 入 beets 库（默认不移动文件）。

    decisions: [{"album": "alb_01", "action": "apply" | "apply_move" |
                 "apply_copy" | "skip",
                 "paths": [...],
                 "candidate": {"album": {...专辑级...}, "mapping": {path: 单曲meta}}}]
    候选数据由 UI 从预览结果原样回传 —— 不重跑网络匹配，保证"所见即所写"；
    这也是为什么 apply 进程不需要与 preview 进程共享 session 状态（进程是一次性的）。

    move/copy 语义：写标签 + 入库后，目标路径冲突的曲目**不覆盖不改名**，
    报 conflict 失败（文件留在原地、标签已写，解决冲突后重新应用即可完成移动）。

    resume（v2）：true 时跳过断点里已完成的 decision（``checkpoints``）。它补的
    真正缺口是 move/copy：中断后已移走的文件不在源路径上，重跑必报
    "源文件不存在"；跳过已完成的，只补剩余的。跳过数进 `resumedSkipped`，
    进度心跳照常（跳过的 decision 也发 progress，UI 才不会像卡住）。

    blankIdentity 门（v2）：合并后标题/专辑/歌手**仍全空**的文件拒绝写入
    （`failed[].code="blankIdentity"`）——落一张连身份都没有的行进库，查重、
    对账、展示全是噪声；宁可让用户先补标签或换候选（借鉴 beets-web-manager
    D3：无证据的 as-is 不落库）。**adopt 不加此门**——原样入库按设计吃
    无标签文件。

    进度**按文件**发（不是按专辑）：宿主有 10 分钟无输出就判卡死的看门狗，
    一张几百首的专辑或慢速磁盘上"专辑级"粒度太粗，会被误杀。
    """
    session = f"app_{req_id:02d}"
    from beets.dbcore import query as dbcore_query
    from beets.library import Item

    load_file_actions()
    lib = open_library()
    done_keys = checkpoints.load() if resume else set()

    active = [d for d in decisions if d.get("action") in ("apply", "apply_move", "apply_copy")]
    apply_total = len(active)
    file_total = sum(len(d.get("paths") or []) for d in active)
    file_done = 0
    emit({"type": "progress", "session": session, "stage": "writing",
           "current": 0, "total": apply_total, "path": None})

    applied = skipped = failed_files = moved_files = resumed_skipped = 0
    for i, dec in enumerate(decisions):
        action = dec.get("action")
        op = FILE_ACTIONS.get(action)
        if action not in ("apply", "apply_move", "apply_copy"):
            skipped += 1
            continue
        paths = [str(p) for p in (dec.get("paths") or [])]
        cand = dec.get("candidate") or {}
        emit({"type": "progress", "session": session, "stage": "writing",
               "current": i + 1, "total": apply_total,
               "path": paths[0] if paths else None})
        key = checkpoints.decision_key(paths, cand)
        if resume and key in done_keys:
            # 断点已完成：不重写、不发 applied，只把进度推走（心跳也在这里）
            resumed_skipped += 1
            continue
        album_meta = cand.get("album") or {}
        mapping = cand.get("mapping") or {}
        written, failures = 0, []
        for p in paths:
            if cancel_gen() != gen:
                emit({"type": "cancelled", "session": session, "stage": "writing"})
                return
            # 逐文件进度：既是 UI 的真实进度，也是看门狗的心跳
            file_done += 1
            emit({"type": "progress", "session": session, "stage": "writing",
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
                # blankIdentity 门（v2）：看**合并后**的结果标签——文件原有
                # 标签可以补候选的缺，反之亦然；合并后仍空白才是真空白。
                if _identity_blank(item):
                    failures.append({
                        "path": p, "code": "blankIdentity",
                        "message": "合并后标签缺失（标题/专辑/歌手为空），未写入。先补标签或换候选。",
                    })
                    continue
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
                    conflict = dest_conflict(mover, p)
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
        emit({"type": "applied", "session": session, "albumId": dec.get("album"),
               "written": written, "moved": op is not None, "failed": failures})
        if not failures:
            # 零失败才记账：有 conflict / blankIdentity 的 decision 重跑还要补
            checkpoints.record(key, dec.get("album"))
    emit({"type": "applyDone", "session": session,
           "applied": applied, "skipped": skipped, "failed": failed_files,
           "moved": moved_files, "resumedSkipped": resumed_skipped})


def _identity_blank(item) -> bool:
    """合并后的身份标签是否空白：标题或专辑为空、且歌手侧全空 → 空白。

    判定用 beets Item 的原始取值（None → 空串），不靠 norm——这里只关心
    "有没有"，不关心"可不可比"。
    """
    def s(field) -> str:
        v = item.get(field)
        if isinstance(v, bytes):
            v = v.decode("utf-8", "replace")
        return str(v or "").strip()

    title, album = s("title"), s("album")
    artist, albumartist = s("artist"), s("albumartist")
    return not title or not album or not (artist or albumartist)
