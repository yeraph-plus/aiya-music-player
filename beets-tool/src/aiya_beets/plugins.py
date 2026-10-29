"""插件能力自检：声明清单 + 三态上报。

## 为什么需要这个模块（docs/31 P0-1）

`config['plugins']` 是**愿望清单**：写着 `lyrics` 不等于 lyrics 能用。
实测（beets 2.14.1）：`plugins.load_plugins()` 对插件导入异常**只写日志、
不抛错**，于是缺 `bs4` 的 lyrics 照样"加载成功"，而真正调用时才炸。
宿主如果按 config 里列的名字判断能力，必然假绿。

所以能力只能自报，且必须三态：
    ready        插件存在 + 依赖齐全 → 真能干活
    missing_deps 插件存在但依赖缺 → 界面上写清楚缺什么
    unavailable  当前 beets 版本里没这个插件（升级/裁剪导致）

检测方式刻意**不 import 插件模块**：`beetsplug.lyrics` 导入失败时
sys.modules 会留下半成品，重试行为不可预测；而且导入有注册钩子副作用。
用 `importlib.util.find_spec` 只看"包在不在"，零副作用。
"""

from __future__ import annotations

import importlib.util
import shutil
from dataclasses import dataclass, field


@dataclass(frozen=True)
class PluginSpec:
    """一个插件声明：叫什么、依赖什么、为什么开它。"""

    name: str
    py_deps: tuple[str, ...] = ()
    exe_deps: tuple[str, ...] = ()
    note: str = ""


@dataclass(frozen=True)
class MarketSpec:
    """市场目录项（docs/40 §3 拍板：白名单，不做全量）。

    `pypi` = None 表示随 beets 内置（免安装，UI 只给开关）；
    否则是 pip 包名（`cmds/market.py` 负责装进当前运行环境）。
    """

    name: str
    note: str
    pypi: str | None = None
    py_deps: tuple[str, ...] = ()
    exe_deps: tuple[str, ...] = ()


# 市场目录：只收录**本地实测过**的有用插件（docs/40 §3.3 风险备注——
# 第三方插件可能拖慢 serve 启动或往 stderr 写杂音，没实测的不收）。
# builtin 档 = beets 自带；pypi 档 = 装进当前运行环境（uv/venv 兼容见 market.py）。
MARKET_CATALOG: tuple[MarketSpec, ...] = (
    MarketSpec("replaygain", "播放音量自动拉齐（引擎直接消费 gain_db）", exe_deps=("ffmpeg",)),
    MarketSpec("lyrics", "歌词抓取（LRC 字段可直供）", pypi="beautifulsoup4", py_deps=("bs4",)),
    MarketSpec("fetchart", "封面抓取（本地缺图时补）", pypi="requests", py_deps=("requests",)),
    MarketSpec("embedart", "封面写进音频文件", exe_deps=("ffmpeg",)),
    MarketSpec("convert", "转码副本（便携设备用）", exe_deps=("ffmpeg",)),
    MarketSpec("scrub", "导入时清掉脏标签"),
    MarketSpec("fromfilename", "无标签文件按文件名兜底猜"),
    MarketSpec("duplicates", "库内查重（维护四件套之一）"),
    MarketSpec("smartplaylist", "beets 查询导出 m3u（给别的设备用）"),
    MarketSpec("edit", "终端外编辑标签（写回库）"),
    MarketSpec("mbsync", "从 MusicBrainz 同步元数据", pypi="requests", py_deps=("requests",)),
    MarketSpec("chroma", "音频指纹识别（acoustid）", pypi="pyacoustid", py_deps=("acoustid",), exe_deps=("fpcalc",)),
)


def find_market_spec(name: str) -> MarketSpec | None:
    """目录查找（大小写敏感：插件名本身全小写）。"""
    for spec in MARKET_CATALOG:
        if spec.name == name:
            return spec
    return None


# 首发档插件（原先是播放器 Rust 里的 FIRST_WAVE_PLUGINS，docs/33 §3.1 搬到这边）。
# 现在每个都带"为什么开它"和"它还需要什么"——这才是能维护的形态。
DEFAULT_PLUGINS: tuple[PluginSpec, ...] = (
    PluginSpec("replaygain", exe_deps=("ffmpeg",), note="提供 gain_db，引擎链路直接消费"),
    PluginSpec("lyrics", py_deps=("bs4",), note="歌词抓取，LRC 字段可直供"),
    PluginSpec("fromfilename", note="无标签文件按文件名兜底猜"),
    PluginSpec("duplicates", note="库内查重（维护四件套之一）"),
    PluginSpec("scrub", note="导入时清掉脏标签"),
    PluginSpec("smartplaylist", note="beets 查询导出 m3u，与推荐页衔接"),
)

READY = "ready"
MISSING_DEPS = "missing_deps"
UNAVAILABLE = "unavailable"


def _has_module(name: str) -> bool:
    """只查"包在不在"，不执行模块代码。"""
    try:
        return importlib.util.find_spec(name) is not None
    except (ImportError, ValueError):
        return False


def capability_report(enabled: list[str] | None = None) -> list[dict]:
    """逐个声明插件给三态。

    `enabled` = config.yaml 里实际写着的插件名（None = 全部声明都算）。
    未在 enabled 里的插件仍然上报（状态里带 `enabled: false`）——
    用户需要知道"有这个东西，但我没开"，而不是从列表里消失。
    """
    enabled_set = set(enabled or [])
    report = []
    for spec in DEFAULT_PLUGINS:
        missing_py = [d for d in spec.py_deps if not _has_module(d)]
        missing_exe = [d for d in spec.exe_deps if shutil.which(d) is None]
        missing = missing_py + missing_exe
        if not _has_module(f"beetsplug.{spec.name}"):
            state = UNAVAILABLE
        elif missing:
            state = MISSING_DEPS
        else:
            state = READY
        report.append(
            {
                "name": spec.name,
                "state": state,
                "enabled": spec.name in enabled_set,
                "missing": missing,
                "note": spec.note,
            }
        )
    return report


def summary(report: list[dict]) -> dict:
    """给 UI 一行话用的计数：ready / missingDeps / unavailable 各几个。

    键名用 camelCase：线上所有其它字段都是 camelCase，这里单独用 snake
    会让宿主 DTO（`rename_all = "camelCase"`）解析失败——而失败路径是
    `from_value(...).ok()`，静默变成 None，UI 只是"什么都没显示"。
    M3 冻结协议时统一，`tests/protocol_contract.py` 现在守着它。
    """
    counts = {"ready": 0, "missingDeps": 0, "unavailable": 0}
    for item in report:
        key = summary_key(item["state"])
        counts[key] = counts.get(key, 0) + 1
    return counts


def summary_key(state: str) -> str:
    """三态 → 汇总计数的键（`missing_deps` → `missingDeps`）。"""
    return "missingDeps" if state == MISSING_DEPS else state


def market_report() -> list[dict]:
    """市场目录的三态报告（`plugin_catalog` 命令的载荷）。

    与 `capability_report` 同一套检测，但覆盖**目录**而非默认清单，
    且多带来源档位（`source: builtin | pypi:<包名>`）——UI 的"安装"按钮
    只对 pypi 档且非 ready 的项出现。
    """
    report = []
    for spec in MARKET_CATALOG:
        missing_py = [d for d in spec.py_deps if not _has_module(d)]
        missing_exe = [d for d in spec.exe_deps if shutil.which(d) is None]
        missing = missing_py + missing_exe
        if not _has_module(f"beetsplug.{spec.name}"):
            state = UNAVAILABLE
        elif missing:
            state = MISSING_DEPS
        else:
            state = READY
        report.append(
            {
                "name": spec.name,
                "note": spec.note,
                "source": "builtin" if spec.pypi is None else f"pypi:{spec.pypi}",
                "state": state,
                "enabled": False,  # 启停归 config（cfg.render_config），目录报告不带
                "missing": missing,
            }
        )
    return report
