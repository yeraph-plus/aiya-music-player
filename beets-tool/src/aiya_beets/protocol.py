"""协议常量与信封构造。

**这是本仓库的契约真源**（docs/33 §4）：命令名、事件名、错误码都在这里定义，
播放器镜像一份 schema 做双侧校验。改这里等于改协议——M3 之后必须同步
`protocol/v1.schema.json`，否则契约测试会红。

协议形态：NDJSON over stdio，一行一条。
    请求  {"id": 1, "cmd": "status"}
    应答  {"id": 1, "ok": true, "data": {...}} | {"id": 1, "ok": false, "error": {...}}
    事件  {"type": "...", ...}   无 id，靠 type 区分
"""

from __future__ import annotations

# 协议版本。**不兼容变更才 +1**：加可选字段不算、加新命令不算、改名算。
PROTOCOL_VERSION = 3

# ---- 错误码（宿主按这个分类给 UX，不按 message 文本匹配）----
E_BAD_JSON = "E_BAD_JSON"            # 请求行不是 JSON
E_BAD_ARGS = "E_BAD_ARGS"            # 参数缺失/形状不对
E_UNKNOWN_CMD = "E_UNKNOWN_CMD"      # 未知命令
E_INTERNAL = "E_INTERNAL"            # 工具内部异常（带 trace）
E_NOT_INSTALLED = "E_NOT_INSTALLED"  # 这个解释器里没有 beets
E_CONFIG = "E_CONFIG"                # 配置读写失败
E_NO_DIRECTORY = "E_NO_DIRECTORY"    # 没显式配置库根（move 拒绝执行）


def ok_response(req_id: int, data) -> dict:
    return {"id": req_id, "ok": True, "data": data}


def error_response(req_id: int, code: str, stage: str, message: str, **extra) -> dict:
    """错误信封。`extra` 用于附带上下文（路径、trace 等），宿主原样展示。"""
    err = {"code": code, "stage": stage, "beets_message": message}
    err.update(extra)
    return {"id": req_id, "ok": False, "error": err}
