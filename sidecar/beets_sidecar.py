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


def handle(req: dict) -> dict:
    cmd = req.get("cmd")
    if cmd == "status":
        return {"ok": True, "data": _status_payload()}
    if cmd == "ping":
        return {"ok": True, "data": {"protocolVersion": PROTOCOL_VERSION}}
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
        sys.stdout.write(json.dumps({"id": req_id, **resp}, ensure_ascii=False) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
