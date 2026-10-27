"""环境自检：这个解释器能不能干活、干得怎么样。

对应原来的 `_status_payload`，但拆成了两层：
    · 本模块负责"问我一个问题，我给你一份体检报告"
    · server 负责"把报告包进协议信封"

体检报告同时是 UI 的数据源，所以键名是协议的一部分（`protocol.py` 是契约真源）。
"""

from __future__ import annotations

import platform

from . import cfg
from .beetslib import configured_directory
from .plugins import capability_report, summary
from .protocol import PROTOCOL_VERSION, SCHEMA_SHA256

# 支持的运行时窗口。下限是语法/标准库要求（`X | None`、tomllib），
# 上限是"没测过的版本不承诺"——不是硬拒绝，只是 status 里标出来。
PYTHON_MIN = (3, 10)
PYTHON_MAX_EXCLUSIVE = (3, 15)

# 声明式依赖：安装器按这个报"还缺什么"，不让用户自己猜
REQUIRED_EXTRAS: dict[str, str] = {
    "bs4": "歌词抓取（lyrics 插件）需要",
}


def runtime_version() -> str:
    return platform.python_version()


def runtime_supported() -> bool:
    info = tuple(int(x) for x in platform.python_version_tuple()[:2])
    return PYTHON_MIN <= info < PYTHON_MAX_EXCLUSIVE


def beets_version() -> str | None:
    """装了就返回版本号，没装返回 None（不抛）。"""
    try:
        import beets

        return getattr(beets, "__version__", None)
    except Exception:  # noqa: BLE001 —— 任何导入期异常都按"不可用"处理
        return None


def is_available() -> bool:
    """beets 能不能用。比 `import beets` 更严：还要能取到 config。"""
    try:
        import beets  # noqa: F401
        import beets.ui  # noqa: F401
        from beets import config as beets_config

        beets_config["library"].as_filename()
        return True
    except (Exception, SystemExit):  # noqa: BLE001 —— beets.ui 导入可能触发 argparse SystemExit
        return False


def status_payload() -> dict:
    """完整体检报告。调用前应确保 `is_available()` 为真。"""
    import beets
    from beets import config as beets_config

    lib_path = beets_config["library"].as_filename()
    plugins = list(beets_config["plugins"].as_str_seq())
    caps = capability_report(plugins)
    return {
        "protocolVersion": PROTOCOL_VERSION,
        # 契约指纹（本模块只报不解释）：播放器拿它跟镜像里的 sha256 比，
        # 一眼看出"装的是不是同一版契约"，比逐字段对更早发现问题
        "schemaHash": SCHEMA_SHA256,
        "beetsVersion": getattr(beets, "__version__", None),
        "pythonVersion": runtime_version(),
        "pythonSupported": runtime_supported(),
        "libraryPath": str(lib_path),
        "libraryExists": _exists(lib_path),
        # 库根目录：**只认 config.yaml 原文**（None = 没显式配过）。
        # 不用 config['directory'] 是因为它恒返回内置默认 ~/Music，
        # UI 拿它显示"整理到 C:\Users\x\Music"会直接误导用户。
        "libraryDirectory": configured_directory(),
        "plugins": plugins,
        "pluginHealth": caps,
        "pluginSummary": summary(caps),
        "configDir": str(beets_config.config_dir()),
        "dataHome": str(cfg.data_home()),
    }


def capabilities_payload() -> dict:
    """`capabilities` 命令的载荷。

    与 `status` 里的 `pluginHealth` 同源（同一个 `capability_report`），
    不维护第二份清单——两份必然漂。
    """
    from beets import config as beets_config

    enabled = list(beets_config["plugins"].as_str_seq())
    caps = capability_report(enabled)
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "schemaHash": SCHEMA_SHA256,
        "plugins": caps,
        "summary": summary(caps),
        "runtimeSupported": runtime_supported(),
        "extras": REQUIRED_EXTRAS,
    }


def _exists(path: str) -> bool:
    import os

    return os.path.exists(path)
