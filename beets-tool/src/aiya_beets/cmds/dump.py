"""把 beets 库全量导出成事件流（宿主用自己的读模型对齐）。"""

from __future__ import annotations

from ..beetslib import open_library
from ..runtime import cancel_gen, emit


# dump_items 输出的字段白名单：只给宿主读模型对齐需要的（beets 字段名原样，
# 宿主侧映射到自有列）。path 是对账 key。
DUMP_FIELDS = [
    "title", "artist", "albumartist", "album", "genre",
    "year", "track", "disc", "length", "bitrate", "format",
    "samplerate", "bitdepth", "channels", "mtime", "rg_track_gain",
]

# 取消检查步长：逐条查 cancel_gen 太碎，200 条查一次在几万条的量级足够即时
_CANCEL_STEP = 200


def dump_items(req_id, gen: int) -> None:
    """流式导出 beets 库 items（宿主读模型对齐用，docs/08 §5 读模型同步）。

    每行一个 item 事件（几万条也不膨胀内存）；最后 dumpDone 带总数。

    v2 补了取消检查——此前是唯一没有 cancel 的流式命令，几万条的库上
    "取消"按钮形同虚设。取消后宿主必须丢弃整份 dump：半份会被误当全量
    对账（宿主已按"没见到 dumpDone 就全弃"兜这个底）。
    """
    session = f"sync_{req_id:02d}"
    lib = open_library()
    total = 0
    for item in lib.items():
        if total % _CANCEL_STEP == 0 and cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "sync"})
            return
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
        emit({"type": "item", "session": session, "item": payload})
    emit({"type": "dumpDone", "session": session, "items": total})
