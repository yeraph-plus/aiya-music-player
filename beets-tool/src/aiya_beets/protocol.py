"""协议常量与信封构造。

**这是本仓库的契约真源**（docs/33 §4）：命令名、事件名、错误码都在这里定义，
播放器镜像一份 schema（`protocol/aiya-beets-v2.json`）做双侧校验。

协议形态：NDJSON over stdio，一行一条。
    请求  {"id": 1, "cmd": "status", "clientProtocol": 2}
    应答  {"id": 1, "ok": true, "data": {...}} | {"id": 1, "ok": false, "error": {...}}
    事件  {"type": "...", ...}   无 id，靠 type 区分

**冻结规则**：加可选字段不算破坏、加新命令/新事件不算破坏、改名算。
改名必须同时改 `protocol/v2.schema.json` 并把 `PROTOCOL_VERSION` +1，
否则契约测试会红（两侧各一条，见 docs/33 §9 M3）。
"""

from __future__ import annotations

# 协议版本。**不兼容变更才 +1**：加可选字段不算、加新命令不算、改名算。
#
# v2（2026-10-29，docs/41 §6）：加法为主（requestId/resume/evidence/libraryLock/
# E_LIBRARY_BUSY/resumedSkipped），拍板升版的理由是新字段在 v2 内定为 required
# （生产端总是计算），版本协商边界兜住旧端；同时接线 schemaHash 运行时比对
# （闭合 docs/35 P1-4）。此后回到增量规则。
#
# 为什么是 1 而不是沿用分包前的 3：分包之前两侧永远同版本一起发，
# 那三个数字是同一份代码内部的迭代次数，从未作为对外契约存在过。
# 冻结成 v1 是给"两个独立发布物"用的**第一版契约**——schema 文件名与
# protocol.version 从此同步演进（现在是 v2），不给未来留"schema 叫 vN
# 但版本号是 M"这种永久解释成本。
PROTOCOL_VERSION = 2

# 本服务端能接受的最低客户端版本。MIN_CLIENT = 2：旧播放器（v1 宿主）会被
# E_PROTOCOL_MISMATCH 拒绝，message 里写明"请升级播放器"——不做降级协商
# （docs/33 §4.4：一半功能能用的模糊状态比失败更难排查）。
MIN_CLIENT = PROTOCOL_VERSION

# schema 文件的字节指纹（`protocol/v2.schema.json`）。
#
# 为什么把它写成常量而不是运行时读文件：**工具装到哪、包怎么打，都不该
# 影响它能不能自报身份**。播放器把镜像里的 sha256 与本值一比，立刻知道
# 对面是同一版契约还是旧组件——不需要读文件、不需要额外命令。
# 改了 schema 忘了改这里？`tests/protocol_contract.py` 会红（它手上真有文件）。
SCHEMA_SHA256 = "72e689f73989a0cd2af6c847bf7a94bb12034d0c466dc4adcbc3b00d4ddae67e"

# ---- 错误码（v1 冻结 10 个，v2 加 E_LIBRARY_BUSY）----
#
# 宿主按 code 分类给 UX，**不按 beets_message 文本匹配**——文本会随人改动。
# 这张表是双向的：schema 里的枚举与本模块必须一字不差，
# `tests/protocol_contract.py` 会断言两边相等（漏一个码就红）。
E_BAD_JSON = "E_BAD_JSON"                        # 请求行不是 JSON
E_INVALID_ARG = "E_INVALID_ARG"                  # 参数缺失/形状不对
E_UNKNOWN_CMD = "E_UNKNOWN_CMD"                  # 未知命令（协议漂移）
E_PROTOCOL_MISMATCH = "E_PROTOCOL_MISMATCH"      # 版本不兼容（往哪边升写在 message 里）
E_NOT_INSTALLED = "E_NOT_INSTALLED"              # 这个解释器里没有 beets
E_NO_LIBRARY_DIR = "E_NO_LIBRARY_DIR"            # 没配库根，move 拒绝执行
E_LIBRARY_DIR_MISMATCH = "E_LIBRARY_DIR_MISMATCH"  # 传入根 ≠ 配置根，拒绝（不猜）
E_BEETS_ERROR = "E_BEETS_ERROR"                  # beets 自己抛的（展示原文）
E_CONFIG = "E_CONFIG"                            # 配置读写失败
E_INTERNAL = "E_INTERNAL"                        # 工具自身 bug（带 trace，请反馈）
E_LIBRARY_BUSY = "E_LIBRARY_BUSY"                # v2：库文件锁被另一进程持有（双开/外部 beet 写库）

ERROR_CODES = frozenset({
    E_BAD_JSON,
    E_INVALID_ARG,
    E_UNKNOWN_CMD,
    E_PROTOCOL_MISMATCH,
    E_NOT_INSTALLED,
    E_NO_LIBRARY_DIR,
    E_LIBRARY_DIR_MISMATCH,
    E_BEETS_ERROR,
    E_CONFIG,
    E_INTERNAL,
    E_LIBRARY_BUSY,
})

# 有意**没有** E_CANCELLED：取消是正常路径，用 `cancelled` 事件表达，
# 不是失败。把取消做成错误码会让宿主的两条路径（事件流 / 一次性应答）
# 互相重叠，且 UI 要为一个"用户自己按的按钮"显示红色。


class ProtocolError(Exception):
    """带错误码的业务异常：命令内部直接抛，由 server 统一转成信封。

    为什么需要它：`move_items` 的三种拒绝理由（无库根 / 根不一致 / 根不存在）
    是**业务判定**，不是 bug。以前它们抛 `ValueError`，被顶层兜底成
    `E_INTERNAL`——宿主只能显示"工具内部异常"，而这些恰恰是宿主该给
    用户明确指引的场景（去配置扫描目录 / 别在未知位置搬文件）。
    """

    def __init__(self, code: str, stage: str, message: str, **extra):
        super().__init__(message)
        assert code in ERROR_CODES, f"未登记的错误码：{code}"
        self.code = code
        self.stage = stage
        self.message = message
        self.extra = extra


def ok_response(req_id: int, data) -> dict:
    return {"id": req_id, "ok": True, "data": data}


def error_response(req_id: int, code: str, stage: str, message: str, **extra) -> dict:
    """错误信封。`extra` 用于附带上下文（路径、trace 等），宿主原样展示。"""
    err = {"code": code, "stage": stage, "beets_message": message}
    err.update(extra)
    return {"id": req_id, "ok": False, "error": err}


def check_client(req: dict) -> None:
    """版本握手：请求带了 `clientProtocol` 就必须与本服务端一致。

    不带 = 信任（两侧同仓同 tag 发版时总是一致的；显式携带是给"用户装了
    新版播放器、旧组件还留着"这种真实场景用的）。

    **不做自动降级协商**（docs/33 §4.4）：不匹配就直接失败，并在 message 里
    说清该往哪边升。协商会产生"一半功能能用"的模糊状态，比失败更难排查。
    """
    raw = req.get("clientProtocol")
    if raw is None:
        return
    try:
        client = int(raw)
    except (TypeError, ValueError):
        raise ProtocolError(
            E_PROTOCOL_MISMATCH, "handshake",
            f"clientProtocol 不是整数：{raw!r}（本组件支持 v{PROTOCOL_VERSION}）",
        ) from None
    if client == PROTOCOL_VERSION:
        return
    if client < PROTOCOL_VERSION:
        raise ProtocolError(
            E_PROTOCOL_MISMATCH, "handshake",
            f"播放器太老（v{client}），本组件支持 v{PROTOCOL_VERSION}：请升级播放器",
            clientProtocol=client,
            serverProtocol=PROTOCOL_VERSION,
        )
    raise ProtocolError(
        E_PROTOCOL_MISMATCH, "handshake",
        f"组件太老（v{PROTOCOL_VERSION}），播放器要求 v{client}：请升级 beets 组件",
        clientProtocol=client,
        serverProtocol=PROTOCOL_VERSION,
    )
