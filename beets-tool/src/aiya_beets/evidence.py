"""候选证据分层（v2）：给每个候选一个"为什么推荐它"的机器可判层。

分层规则借鉴 beets-web-manager 的 ADR 0002/0007（docs/41 §3.5），自强到弱：
`recordingId > metadataSlot > releaseGroup > similarity`。

三条从对方踩坑里学来的纪律：

1. 标题归一化**版本保持**——不剥 ``(Live)`` / remaster / ``feat.`` 括注。
   剥了的话 "Song (Live 2019)" 与错歌 "Song" 归一同形，错歌也能拿满分；
   所以这里的归一化只做 casefold + 空白折叠。
2. 双源印证缺任何一源就 fail closed 到下一层——原始文件名与嵌入标签
   是两个独立来源，只信其一等于没印证。
3. ``fromScratch`` 短路为 similarity：它没有外部证据，"标题全匹配"是它
   跟自己比，不算证据。

指纹层（AcoustID/chroma）预留不实现：那依赖 chroma 插件启用情况，现在没有
可靠途径拿到"指纹一致"的事实——宁可缺层，不虚标。
"""

from __future__ import annotations

import os
import re

# 时长容差（秒）：同一曲目的两次 rip 时长差普遍 <1s，2s 是保守上限
DURATION_TOLERANCE_S = 2.0

# "01 - " / "01." / "01_" / "01 " 这类开头的曲目号前缀
_LEADING_TRACKNO = re.compile(r"^\s*\d{1,3}[\s._\-]+")

# 层级常量（与 schema 的 EvidenceTier 枚举一字不差）
RECORDING_ID = "recordingId"
METADATA_SLOT = "metadataSlot"
RELEASE_GROUP = "releaseGroup"
SIMILARITY = "similarity"


def norm_title(value) -> str:
    """版本保持归一化：casefold + 空白折叠。**刻意不剥括注**（纪律 1）。"""
    if value is None:
        return ""
    if isinstance(value, bytes):
        value = value.decode("utf-8", "replace")
    return " ".join(str(value).split()).casefold()


def filename_stem(path: str) -> str:
    """原始文件名主干：去扩展名、剥开头曲目号。

    **必须喂原始路径**（decisions 里的源路径），不能喂重命名后的——
    重命名是本工具自己写的目标名，拿它对答案是循环论证（BWM 实测踩过的坑）。
    """
    stem = os.path.splitext(os.path.basename(path))[0]
    return _LEADING_TRACKNO.sub("", stem).strip()


def _as_int(value):
    """beets 的 track/disc 可能是 int、int 形 float、str（黑胶 "A1"）或 None。
    转不了的返回 None——位置证据 fail closed。"""
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def _pair_length(item, info) -> tuple[float, float] | None:
    """双方时长；任何一侧缺失/非数返回 None（fail closed）。"""
    try:
        ilen = float(item.length or 0)
        flen = float(getattr(info, "length", None) or 0)
    except (TypeError, ValueError):
        return None
    if ilen <= 0 or flen <= 0:
        return None
    return ilen, flen


def compute(match) -> tuple[str, list[str]]:
    """AlbumMatch → (证据层, 依据短句)。任何分支都不抛——证据是展示辅助，
    它挂了最多退回 similarity，不能连累整个 preview。"""
    try:
        return _compute(match)
    except Exception:  # noqa: BLE001 —— 见上：降级不连累主流程
        return SIMILARITY, ["证据计算异常，按相似度处理"]


def _compute(match) -> tuple[str, list[str]]:
    if getattr(match, "from_scratch", False):
        return SIMILARITY, ["无外部匹配，按现有标签入库（fromScratch）"]
    pairs = list(getattr(match, "item_info_pairs", None) or [])
    if not pairs:
        return SIMILARITY, ["无逐曲对齐数据"]

    # ---- recordingId：全部对齐曲的嵌入 Recording ID 与候选一致 ----
    rec_total = rec_hit = 0
    for item, info in pairs:
        item_mb = item.get("mb_trackid")
        info_mb = getattr(info, "track_id", None)
        if item_mb and info_mb:
            rec_total += 1
            if norm_title(item_mb) == norm_title(info_mb):
                rec_hit += 1
    if rec_total and rec_hit == rec_total:
        return RECORDING_ID, [
            f"嵌入的 MusicBrainz Recording ID 与候选完全一致（{rec_hit}/{rec_total} 首）"
        ]

    # ---- metadataSlot：双源印证 + 精确碟轨位置 + 时长容差，全部对齐曲满足 ----
    slot_ok = True
    for item, info in pairs:
        info_title = norm_title(getattr(info, "title", None))
        if not info_title:
            slot_ok = False
            break
        path = item.path.decode("utf-8", "replace") if isinstance(item.path, bytes) else str(item.path)
        if norm_title(item.get("title")) != info_title:
            slot_ok = False  # 源 1：嵌入标签
            break
        if norm_title(filename_stem(path)) != info_title:
            slot_ok = False  # 源 2：原始文件名（独立来源）
            break
        want_disc = _as_int(getattr(info, "medium", None))
        want_track = _as_int(getattr(info, "medium_index", None))
        if want_track is None:
            want_track = _as_int(getattr(info, "index", None))
        if _as_int(item.get("disc")) != want_disc or _as_int(item.get("track")) != want_track:
            slot_ok = False
            break
        lengths = _pair_length(item, info)
        if lengths is None or abs(lengths[0] - lengths[1]) > DURATION_TOLERANCE_S:
            slot_ok = False
            break
    if slot_ok:
        return METADATA_SLOT, [
            f"原始文件名与嵌入标签双重印证（{len(pairs)} 首），碟轨位置与时长全部对上"
        ]

    # ---- releaseGroup：候选锚定 Release Group ID（专辑级身份，ADR 0002） ----
    rg = getattr(getattr(match, "info", None), "releasegroup_id", None)
    if rg:
        rg = str(rg)
        return RELEASE_GROUP, [f"候选锚定 Release Group {rg[:8]}…"]

    return SIMILARITY, ["仅相似度匹配（距离评分），无确定身份证据"]
