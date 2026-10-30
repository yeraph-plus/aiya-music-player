"""插件能力：一份 JSON 表 + 三态自检 + 市场目录。

## 为什么需要三态（docs/31 P0-1）

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

## 为什么表在 JSON 里（docs/51 §1）

这张表原本有三份硬编码：`MARKET_CATALOG`（市场目录 12 条）、
`DEFAULT_PLUGINS`（首发 6 条）、`INDEX_PLUGINS`（索引家白名单 3 条），
外加 `env.REQUIRED_EXTRAS` 一处人工说明。同一件事写四遍，必然漂——
而且 `install.py` 与 `build_bundle.py` 各自不知道对方装了哪些包。

现在唯一事实源是 `data/plugins.json`，五个消费方读同一份字节：

    plugins.py         → 三个派生视图（本模块）
    install.py         → --plugins a,b 拼 pip 命令
    build_bundle.py    → bundle:true 的包预置进免配置包
    src-tauri/build.rs → include_str! 进二进制，UI 未装组件也能勾选
    cfg.py             → 索引家白名单的强制参数与结构拒绝

读取用 `importlib.resources` 而不是 `Path(__file__)`：`.pyz` 是 zip，
`Path(__file__).parent/'data'` 在 zip 里 `is_file()` 恒 False
（这就是 A 轨道以前"缺少依赖清单"的真因，docs/51 §0）。
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
from dataclasses import dataclass
from importlib.resources import files
from typing import Any


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

    `packages` 为空 = 不需要额外 pip 包（要么随 beets 内置，要么依赖已在
    `requirements.lock` 里，比如 fetchart 要的 requests）。UI 的「安装」按钮
    只对 `packages` 非空且非 ready 的项出现。
    """

    name: str
    note: str
    packages: tuple[str, ...] = ()
    py_deps: tuple[str, ...] = ()
    exe_deps: tuple[str, ...] = ()

    @property
    def pypi(self) -> str | None:
        """第一个 pip 包名（老调用方的兼容视图；新代码用 `packages`）。"""
        return self.packages[0] if self.packages else None


@dataclass(frozen=True)
class BinarySpec:
    """一个外部二进制：从哪拿、多大、什么哈希、落到 bin/ 的什么名字（docs/53）。

    ## 为什么"载体"和"落盘物"是两个东西

    我们要的不是"装个包"，而是 **`bin/` 下有个固定名字的 exe**：PATH 注入、
    状态显示、卸载清理都靠这个路径。而载体（wheel / zip）内部的文件名带版本、
    还带别的文件。所以声明里两套尺寸/哈希都要有：

        carrier.size / carrier.sha256  →  下载完立刻验（不等解压）
        member                         →  从里面按 glob 抽哪一个
        size / sha256                  →  抽出来的 exe 再验一次

    ffmpeg 的载体就是 PyPI 的 `imageio-ffmpeg` 轮子 —— 轮子本身就是 zip，
    所以 `kind` 只有 `zip` 一种，不另设 `pypi-wheel`：多一个分支只多一处
    要维护的走法，换来的是同一件事的两种写法。
    """

    name: str
    target: str          # 落到 bin/ 下的文件名（固定，不跟载体内部名字走）
    url: str
    carrier_size: int
    carrier_sha256: str
    member: str          # 载体内的成员，允许 glob（必须恰好命中 1 个）
    size: int
    sha256: str
    probe: tuple[str, ...] = ()
    # essentia 那个 exe 无参调用退出码非 0，但 stderr 打 Usage —— 那也算"能跑"
    probe_allow_fail: bool = False
    license: str = ""
    vendor: str = ""
    why: str = ""


@dataclass(frozen=True)
class IndexPluginSpec:
    """索引家（DB1）允许的插件：**名字 + 强制参数 + 写盘标志**三元组。

    ## 为什么不能只有名字（docs/45 §6.2）

    白名单只写名字，挡不住"插件默认值自己写盘"这件事。实测（docs/45 §3.7）：
    `xtractor` 的 `write` 默认就是 `yes`，命中即 `item.try_write()` 把 bpm
    写回音频文件——一句话就破掉"索引家永不改文件"的前提，而且没有任何报错。

    所以每个条目必须带 `forced`：渲染配置时把这些键值原样写进 config.yaml，
    渲染后再由 `cfg.verify_config` 反向断言。`writes_files=True` 的插件
    永远不许出现在索引家（`embedart` / `convert` / `scrub` 属这一类），
    由白名单结构本身拒绝，而不是靠调用方记得别加。

    在 JSON 里这一条表达为「有 `indexForced` 才可能进 `profiles.index`」，
    并由契约测试守着两条不变量：白名单里的人必须有 `indexForced`，
    且「默认会写盘」的插件其 `indexForced` 必须把 `write` 压成 `no`。
    """

    name: str
    forced: tuple[tuple[str, str], ...] = ()
    writes_files: bool = False
    needs_network: bool = False
    needs_exe: tuple[str, ...] = ()
    note: str = ""


# ---------------------------------------------------------------- 表加载

_CATALOG: dict[str, Any] | None = None


def catalog() -> dict[str, Any]:
    """解析后的 `plugins.json`（进程内缓存）。"""
    global _CATALOG
    if _CATALOG is None:
        _CATALOG = json.loads(catalog_text())
    return _CATALOG


def catalog_text() -> str:
    """原始 JSON 文本（宿主/Rust 侧校验用，不做二次序列化）。"""
    return files("aiya_beets").joinpath("data/plugins.json").read_text(encoding="utf-8")


def entries() -> list[dict[str, Any]]:
    """目录条目（顺序 = JSON 里的顺序 = UI 展示顺序）。"""
    return list(catalog()["plugins"])


def entry(name: str) -> dict[str, Any] | None:
    """按名字查一条（大小写敏感：插件名本身全小写）。"""
    for item in entries():
        if item["name"] == name:
            return item
    return None


# ---------------------------------------------------------------- 派生视图

MARKET_CATALOG: tuple[MarketSpec, ...] = tuple(
    MarketSpec(
        name=item["name"],
        note=item.get("note", ""),
        packages=tuple(item.get("packages", ())),
        py_deps=tuple(item.get("imports", ())),
        exe_deps=tuple(item.get("exe", ())),
    )
    for item in entries()
)

DEFAULT_PLUGINS: tuple[PluginSpec, ...] = tuple(
    PluginSpec(
        name=name,
        py_deps=tuple((entry(name) or {}).get("imports", ())),
        exe_deps=tuple((entry(name) or {}).get("exe", ())),
        note=(entry(name) or {}).get("note", ""),
    )
    for name in (catalog()["profiles"]["workbench"])
)

INDEX_PLUGINS: tuple[IndexPluginSpec, ...] = tuple(
    IndexPluginSpec(
        name=item["name"],
        forced=tuple(item.get("indexForced", {}).items()),
        # 有 indexForced 才可能在白名单里；只要带了写盘能力，就必须由
        # indexForced 显式压住（契约测试守着，这里再兜一次运行期）
        writes_files=bool(item.get("writes")) and "write" not in item.get("indexForced", {}),
        needs_network=bool(item.get("network")),
        needs_exe=tuple(item.get("exe", ())),
        note=item.get("note", ""),
    )
    for item in entries()
    if item.get("indexForced") is not None
)

INDEX_DEFAULT_PLUGINS: tuple[str, ...] = tuple(catalog()["profiles"]["index"])
WORKBENCH_DEFAULT_PLUGINS: tuple[str, ...] = tuple(catalog()["profiles"]["workbench"])

# 免配置包预置的插件（`bundle: true`）。集合本身是产品决策，改 JSON 不改代码。
BUNDLE_PLUGINS: tuple[str, ...] = tuple(item["name"] for item in entries() if item.get("bundle"))

BINARIES: tuple[BinarySpec, ...] = tuple(
    BinarySpec(
        name=item["name"],
        target=item["target"],
        url=item["carrier"]["url"],
        carrier_size=int(item["carrier"]["size"]),
        carrier_sha256=item["carrier"]["sha256"],
        member=item["carrier"]["member"],
        size=int(item["size"]),
        sha256=item["sha256"],
        probe=tuple(item.get("probe", ())),
        probe_allow_fail=bool(item.get("probeAllowFail", False)),
        license=item.get("license", ""),
        vendor=item.get("vendor", ""),
        why=item.get("why", ""),
    )
    for item in (catalog().get("binaries") or ())
)


def binary(name: str) -> BinarySpec | None:
    """按名字查外部二进制声明。"""
    for spec in BINARIES:
        if spec.name == name:
            return spec
    return None


def binaries_for(names: list[str]) -> list[str]:
    """这些插件要哪些外部二进制（去重保序）。

    未知插件名抛 `KeyError`，与 `packages_for` 同一约定：静默跳过等于
    "勾了 A 装了 B"。名字不在 `binaries` 表里同样抛 `KeyError` —— 那是表自己
    写错了，必须当场发现，而不是等到安装时"少下一个文件"。
    """
    out: list[str] = []
    for name in names:
        item = entry(name)
        if item is None:
            raise KeyError(name)
        for dep in item.get("exe", ()):
            if dep not in out:
                if binary(dep) is None:
                    raise KeyError(dep)
                out.append(dep)
    return out


def exe_path(dep: str) -> str | None:
    """这个外部依赖现在能不能找到：`bin/` 优先，其次 PATH。

    为什么要先看 `bin/`：`bin/` 是我们自己下发的（docs/53），它不需要 PATH
    注入就已经存在；而 `shutil.which` 只在 `cli.main` 注入之后才看得见它。
    两边都认，配置渲染与三态报告才能在建进程之前就得出正确结论。

    beets 侧真正调用时走的是 PATH（replaygain 写死 `"ffmpeg"`），那一步由
    `bins.inject_path()` 负责 —— 这里只回答"有没有"。
    """
    from . import bins  # 延迟导入：bins 顶层就 import 本模块（避免环）

    local = bins.bin_dir()
    if local is not None:
        candidates = [dep]
        target = bin_target(dep)
        if target:
            candidates.insert(0, target)
        if os.name == "nt":
            candidates.append(dep + ".exe")
        for name in candidates:
            found = local / name
            if found.is_file():
                return str(found)
    return shutil.which(dep)


def bin_target(dep: str) -> str | None:
    """依赖名 → `bin/` 下的文件名（`ffmpeg` → `ffmpeg.exe`）。"""
    spec = binary(dep)
    return spec.target if spec else None


def missing_binaries(deps: list[str]) -> list[str]:
    """这些外部依赖里，现在还缺哪几个。"""
    return [dep for dep in deps if exe_path(dep) is None]


def usable_plugins(names: list[str]) -> list[str]:
    """把"外部依赖还缺"的插件从清单里剔掉，保留原顺序。

    ## 为什么必须剔（docs/53 §1.1 实测）

    `plugins: replaygain` 而 PATH 上没有 ffmpeg 时，beets **任何子命令**都
    起不来：插件在启动时初始化，`FatalReplayGainError` 被包成 `UserError`
    直接抛。所以"写了但缺 exe"不是"该功能降级"，是"整个组件不可用"。

    剔除发生在渲染配置的这一侧，而不是让人去记得别勾 —— 老配置、用户手删
    `bin/`、从别的机器拷来的 `settings.json` 都会走到这里。被剔掉的插件在
    三态报告里仍然是 `missing_deps`（缺 `ffmpeg`），UI 上不会凭空消失。
    """
    out: list[str] = []
    for name in names:
        item = entry(name)
        if item is None:
            # 不认识的名字原样保留：workbench 是用户的家，他写什么就是什么
            out.append(name)
            continue
        if missing_binaries(list(item.get("exe", ()))):
            continue
        out.append(name)
    return out



def find_market_spec(name: str) -> MarketSpec | None:
    """目录查找。"""
    for spec in MARKET_CATALOG:
        if spec.name == name:
            return spec
    return None


def find_index_plugin(name: str) -> IndexPluginSpec | None:
    """按名字查索引家白名单。"""
    for spec in INDEX_PLUGINS:
        if spec.name == name:
            return spec
    return None


def packages_for(names: list[str]) -> list[str]:
    """把插件名解析成 pip 包清单（去重保序）。

    这是 `install.py --plugins` 的唯一实现：脚本不自己拼依赖，
    所以"这个插件要装什么"只有 JSON 一处。

    未知名字抛 `KeyError`——调用方必须把它转成可读错误。
    静默跳过会让用户"勾了 A 装了 B"，比报错难查十倍。
    """
    seen: list[str] = []
    for name in names:
        item = entry(name)
        if item is None:
            raise KeyError(name)
        for pkg in item.get("packages", ()):
            if pkg not in seen:
                seen.append(pkg)
    return seen


def imports_for(names: list[str]) -> list[str]:
    """这些插件各自的 Python 模块名（自检用，去重）。"""
    out: list[str] = []
    for name in names:
        item = entry(name)
        if item is None:
            raise KeyError(name)
        for mod in item.get("imports", ()):
            if mod not in out:
                out.append(mod)
    return out


# ---------------------------------------------------------------- 三态

READY = "ready"
MISSING_DEPS = "missing_deps"
UNAVAILABLE = "unavailable"


def _has_module(name: str) -> bool:
    """只查"包在不在"，不执行模块代码。"""
    try:
        return importlib.util.find_spec(name) is not None
    except (ImportError, ValueError):
        return False


def _state_of(py: tuple[str, ...], exe: tuple[str, ...], plugin: str) -> tuple[str, list[str]]:
    """三态 + 缺什么。`capability_report` 与 `market_report` 共用，避免各算一套。

    外部依赖走 [`exe_path`] 而不是 `shutil.which`：`bin/` 里的东西在 PATH 注入
    之前也该算"有"（配置渲染就发生在注入之前）。
    """
    missing_py = [d for d in py if not _has_module(d)]
    missing_exe = [d for d in exe if exe_path(d) is None]
    missing = missing_py + missing_exe
    if not _has_module(f"beetsplug.{plugin}"):
        return UNAVAILABLE, missing
    if missing:
        return MISSING_DEPS, missing
    return READY, missing


def capability_report(enabled: list[str] | None = None) -> list[dict]:
    """逐个声明插件给三态。

    `enabled` = config.yaml 里实际写着的插件名（None = 全部声明都算）。

    覆盖面是 **默认清单 ∪ 已启用清单**（按目录顺序）：既有"有这个东西但我没开"
    的可发现性，也不让"用户勾过的非默认插件"（replaygain / chroma / convert）
    从状态里消失 —— 而恰恰是它们要外部二进制，最需要被看见缺什么。
    """
    enabled_set = set(enabled or [])
    tracked = set(WORKBENCH_DEFAULT_PLUGINS) | enabled_set
    report = []
    for item in entries():
        if item["name"] not in tracked:
            continue
        spec = MarketSpec(
            name=item["name"],
            note=item.get("note", ""),
            packages=tuple(item.get("packages", ())),
            py_deps=tuple(item.get("imports", ())),
            exe_deps=tuple(item.get("exe", ())),
        )
        state, missing = _state_of(spec.py_deps, spec.exe_deps, spec.name)
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
    且多带来源档位（`source: builtin | pypi:<包名>`）与 `packages`
    （UI 显示"要装什么"、安装脚本拼命令都用它）。
    """
    report = []
    for spec in MARKET_CATALOG:
        state, missing = _state_of(spec.py_deps, spec.exe_deps, spec.name)
        report.append(
            {
                "name": spec.name,
                "note": spec.note,
                "source": "builtin" if not spec.packages else "pypi:" + ",".join(spec.packages),
                "packages": list(spec.packages),
                "state": state,
                "enabled": False,  # 启停归 config（cfg.render_config），目录报告不带
                "missing": missing,
            }
        )
    return report


def ui_catalog() -> list[dict]:
    """给宿主/UI 的静态目录：不依赖 beets 运行时，也不探测三态。

    宿主用的是编译期 `include_str!` 的那份 JSON（`beets_plugin_catalog`），
    这里保留同形状的入口是为了两条路径（未装组件 / 已装组件）说话一致。
    """
    rows = []
    workbench = set(WORKBENCH_DEFAULT_PLUGINS)
    for item in entries():
        rows.append(
            {
                "name": item["name"],
                "title": item.get("title", item["name"]),
                "note": item.get("note", ""),
                "builtin": bool(item.get("builtin", True)),
                "packages": list(item.get("packages", ())),
                "exe": list(item.get("exe", ())),
                "writes": bool(item.get("writes", False)),
                "network": bool(item.get("network", False)),
                "bundle": bool(item.get("bundle", False)),
                "default": item["name"] in workbench,
                "schema": catalog()["schema"],
            }
        )
    return rows
