"""配置生成：`config.yaml` 长什么样、放在哪，全归这里。

## 为什么这属于工具侧（docs/33 §2.1）

YAML 键名（`library` / `directory` / `plugins`）、单引号转义规则、哪个键可以省——
这些都是 beets 的知识。播放器不该知道，否则 beets 改配置格式就得跟着改播放器。
现在播放器只提供两个**事实**，通过环境变量传进来：

    AIYA_BEETS_HOME       数据根（播放器给 `{app_data}/beets` 或 `{app_data}/beets-index`）
    AIYA_BEETS_SCAN_ROOT  用户的第一个扫描根（可选；没给就不写 directory）
    AIYA_BEETS_PROFILE    `workbench`（默认，用户库）| `index`（索引库，docs/45 §4.1）

## 两个刻意的"不"

1. **已存在的 config.yaml 一个字都不动**（仅 `workbench` 家）。用户手改过的配置
   不能被应用每次启动覆盖；要恢复默认就删掉这个文件。
2. **没有扫描根时省略 directory 行**（仅 `workbench` 家）。瞎猜一个目录比不写更糟
   ——猜错等于把用户的文件 move 到他没预期的地方（`config['directory']` 恒有内置
   默认 `~/Music`，所以"不写"和"写默认"是两件事）。

## 索引家为什么反过来（docs/45 §6.1）

`index` 家的 config.yaml 是**生成的工件，不是用户配置**：它承载"永不写文件"这条
安全前提（`import.write/copy/move: no` + `xtractor.write: no` + 插件白名单），
而这几条全靠 beets 的默认值恰好不对——xtractor 的 `write` 默认就是 `yes`。
所以这一侧：

* `directory` 恒指向 `{home}/scratch` 这个**占位空目录**（实测：指向扫描根时根内
  文件在库里存相对路径，`path::` 定位失效，清旧步骤就做不准）；
* 每次启动都按模板重渲染（内容相同则不动盘），**渲染后自检**——断言关键键存在
  且为期望值，不依赖"默认值恰好对"；
* 用户在 UI 里没有编辑入口（拍板 D），要改就改模板。

自检的意义不在"模板写对了没有"（那是同义反复），而在于**任意一份落盘的文本**
都必须过闸：老版本写的、手改的、被别的工具动过的，过不去就重写。测试里的负向
验证正是往文本里改一个 `write: yes`，闸必须变红。
"""

from __future__ import annotations

import os
from pathlib import Path

from .plugins import (
    DEFAULT_PLUGINS,
    INDEX_DEFAULT_PLUGINS,
    INDEX_PLUGINS,
    find_index_plugin,
)

PROFILE_WORKBENCH = "workbench"
PROFILE_INDEX = "index"
PROFILES = (PROFILE_WORKBENCH, PROFILE_INDEX)

# 索引家的占位整理根目录名（在 home 下，实测只有"空目录"这一个要求）
SCRATCH_DIR = "scratch"

# 索引家硬钉的 beets 键值。顺序即渲染顺序（自检也按这张表走，两边不会漂）。
INDEX_IMPORT_KEYS: tuple[tuple[str, str], ...] = (
    ("write", "no"),      # 不写回标签
    ("copy", "no"),       # 不复制进库
    ("move", "no"),       # 不移动文件（这三条是"文件不动"的正文）
    ("incremental", "yes"),
    ("quiet", "yes"),
    ("duplicate_action", "keep"),  # 默认 ask 会吞掉同标签同长的第二个文件
)


class ConfigError(Exception):
    """配置层的错误基类。"""


class ConfigConflict(ConfigError):
    """环境自相矛盾（`BEETSDIR` 与 `AIYA_BEETS_HOME` 分家、profile 不认识）。

    宁可启动失败，也不要两家库悄悄合流——合流后的现象是"索引明明跑完了，
    查询却什么都没有"，排查成本远高于一条明确的启动错误。
    """


class ConfigGateError(ConfigError):
    """落盘的索引家配置没过自检（写盘开关被改、插件越界、占位根不对）。"""


def _norm(p: str | Path) -> str:
    """路径比较口径：展开 ~、绝对化、Windows 下大小写归一。

    不用 `resolve()`：它会解析符号链接（用户把 home 链到别的盘时反而对不上），
    而且对不存在的路径行为随版本变过。这里要的是"看起来是不是同一个位置"。
    """
    return os.path.normcase(os.path.abspath(os.path.expanduser(str(p))))


def _home_env() -> str | None:
    """`AIYA_BEETS_HOME`（新名）优先，`AIYA_BEETS_DIR`（dev 脚本的旧名）兜底。"""
    for key in ("AIYA_BEETS_HOME", "AIYA_BEETS_DIR"):
        val = os.environ.get(key)
        if val:
            return val
    return None


def data_home() -> Path:
    """数据根：环境变量优先，否则用 beets 自己的默认配置目录。"""
    val = _home_env()
    if val:
        return Path(val)
    from beets import config as beets_config

    return Path(str(beets_config.config_dir()))


def profile() -> str:
    """当前实例是哪个家。未设置 = `workbench`（行为与 v1 完全一致）。

    未知值直接拒绝：这个变量只由宿主下发，写错就是宿主的编程错误，
    静默回落到 workbench 会让索引会话写进用户库——那是最坏的一种"猜"。
    """
    raw = (os.environ.get("AIYA_BEETS_PROFILE") or "").strip().lower()
    if not raw:
        return PROFILE_WORKBENCH
    if raw not in PROFILES:
        raise ConfigConflict(
            f"AIYA_BEETS_PROFILE={raw!r} 不认识（可选：{', '.join(PROFILES)}）"
        )
    return raw


def apply_env() -> None:
    """把 `AIYA_BEETS_HOME` 翻译成 beets 认的 `BEETSDIR`，并校验两者不打架。

    **必须在任何 `import beets` 之前调用**：beets 在导入时就把 config_dir
    定下来了，之后再改环境变量没有用（实测：只设 AIYA_BEETS_HOME 时，
    config.yaml 写在自定义目录，而库文件仍落在 `%APPDATA%\\beets` —— 两处分裂，
    用户看到的是"配置生效了但库不对"）。

    v2 起 `BEETSDIR` 不再"显式给的优先、其余不管"（docs/45 §6.3）：双家隔离靠
    `AIYA_BEETS_HOME` 切家，而锁文件、ops 登记簿、断点都按 `data_home()` 定位。
    外部环境里残留的 `BEETSDIR` 会让 **beets 写 A 家、锁建在 B 家**，隔离静默
    失效——所以两者都在且不等时直接抛 `ConfigConflict`，fail-fast 不猜。
    """
    beetsdir = os.environ.get("BEETSDIR")
    home = _home_env()
    if beetsdir and home and _norm(beetsdir) != _norm(home):
        raise ConfigConflict(
            "BEETSDIR 与 AIYA_BEETS_HOME 指向不同的家，拒绝启动以免写错库："
            f"BEETSDIR={beetsdir}  AIYA_BEETS_HOME={home}。"
            "两者必须一致（双家隔离依赖它们同步；清掉其中一个即可）。"
        )
    if beetsdir:
        return
    if home:
        os.environ["BEETSDIR"] = str(Path(home).expanduser())


def scan_root() -> str | None:
    """用户第一个扫描根（播放器传的事实）；空串/空白视为没给。"""
    val = (os.environ.get("AIYA_BEETS_SCAN_ROOT") or "").strip()
    return os.path.expanduser(val) if val else None


def yaml_path(p: str | Path) -> str:
    """YAML 单引号字符串：里面的反斜杠是字面量，Windows 路径不用再转义。

    双引号里 `\\M` 会被当成转义序列——这是这类配置文件最经典的坑。
    """
    return "'" + str(p).replace("'", "''") + "'"


def scratch_dir(home: Path) -> Path:
    """索引家的占位整理根（空目录，永不承载文件）。"""
    return home / SCRATCH_DIR


def render_config(
    home: Path,
    root: str | None,
    plugins: list[str] | None = None,
    profile: str = PROFILE_WORKBENCH,
) -> str:
    """生成 config.yaml 正文（纯函数，可单测）。

    `workbench`：刻意只有三行——library、可选 directory、plugins。beets 的其余
    配置保持它自己的默认（用户的家，用户做主）。

    `index`：除了上面三行，还钉死 `import` 段的写盘开关与每个插件的强制参数
    （docs/45 §6.1）。`root` 在这一侧**被忽略**——整理根恒为占位目录。
    """
    if profile not in PROFILES:
        raise ConfigConflict(f"未知 profile: {profile!r}")
    if profile == PROFILE_INDEX:
        return _render_index(home, plugins)
    return _render_workbench(home, root, plugins)


def _render_workbench(home: Path, root: str | None, plugins: list[str] | None) -> str:
    names = plugins if plugins is not None else [p.name for p in DEFAULT_PLUGINS]
    lines = [
        "# AIYA Music 生成；改过之后不会再被覆盖（要恢复默认就删掉本文件）",
        f"library: {yaml_path(home / 'library.db')}",
    ]
    if root:
        lines.append(f"directory: {yaml_path(root)}")
    lines.append("plugins: " + " ".join(names))
    return "\n".join(lines) + "\n"


def _render_index(home: Path, plugins: list[str] | None) -> str:
    names = list(plugins) if plugins is not None else list(INDEX_DEFAULT_PLUGINS)
    lines = [
        "# AIYA Music 索引库（DB1）：这是生成物，不要手改——改模板（cfg.render_config）。",
        "# 可随时重建：删掉 library.db* 与 state.pickle **成对删**（state 才是增量记忆）。",
        f"library: {yaml_path(home / 'library.db')}",
        f"directory: {yaml_path(scratch_dir(home))}",
        "plugins: " + " ".join(names),
        "import:",
    ]
    lines += [f"    {key}: {value}" for key, value in INDEX_IMPORT_KEYS]
    # 每个启用插件的强制参数（xtractor 的 write: yes 会把 bpm 写回音频文件）
    for name in names:
        spec = find_index_plugin(name)
        if spec is None or not spec.forced:
            continue
        lines.append(f"{name}:")
        lines += [f"    {key}: {value}" for key, value in spec.forced]
    return "\n".join(lines) + "\n"


def _parse(text: str) -> dict[str, object]:
    """极简 YAML 子集解析：只认"顶层 `key: value`"与"4 空格缩进的子键"。

    为什么自己解析而不是 `yaml.safe_load`：门禁要在**任何**落盘文本上工作，
    包括被人手改坏的；用完整解析器会因为一处语法错误抛异常，而我们要的是
    "这条键不对"这种可读结论。只解析我们自己渲染的那几种形状就够了，
    解析不出来的形状一律视为不合规（拒绝比宽容安全）。
    """
    top: dict[str, object] = {}
    current: dict[str, str] | None = None
    for raw in text.splitlines():
        line = raw.rstrip()
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        stripped = line.lstrip()
        indent = len(line) - len(stripped)
        if indent == 0:
            key, _, value = stripped.partition(":")
            key, value = key.strip(), value.strip()
            if value:
                top[key] = value
                current = None
            else:
                current = {}
                top[key] = current
        elif current is not None and indent > 0:
            key, _, value = stripped.partition(":")
            current[key.strip()] = value.strip()
        else:
            raise ConfigGateError(f"无法识别的配置行（缩进/位置不对）：{raw!r}")
    return top


def _plugin_names(value: object) -> list[str]:
    """`plugins:` 的两种写法都收：`xtractor lastgenre` 与 `[xtractor, lastgenre]`。"""
    if value is None:
        return []
    text = str(value).strip().strip("[]")
    return [part for part in text.replace(",", " ").split() if part]


def verify_config(text: str, profile: str = PROFILE_INDEX, home: Path | None = None) -> None:
    """索引家配置的自检闸。不合规抛 `ConfigGateError`，消息说清哪一条不对。

    `workbench` 家不设闸：那是用户的家，任何写法都合法。
    """
    if profile != PROFILE_INDEX:
        return
    doc = _parse(text)

    if not doc.get("library"):
        raise ConfigGateError("缺少 library（索引家必须有独立的库文件）")

    # 占位整理根：必须是 home/scratch，且不能是扫描根（相对路径会让 path 定位失效）
    want_dir = _norm(scratch_dir(home)) if home is not None else None
    if not doc.get("directory"):
        raise ConfigGateError("缺少 directory（索引家必须是占位空目录，不能省）")
    got_dir = _norm(str(doc["directory"]).strip("'\""))
    if want_dir is not None and got_dir != want_dir:
        raise ConfigGateError(
            f"directory 必须是占位目录 {scratch_dir(home)}，实际是 {doc['directory']}"
            "（指向扫描根会让库内存相对路径，path 定位失效）"
        )

    # 插件白名单三元组：名字在白名单内、不写盘、强制参数齐全
    names = _plugin_names(doc.get("plugins"))
    for name in names:
        spec = find_index_plugin(name)
        if spec is None:
            raise ConfigGateError(f"插件 {name} 不在索引家白名单里（{_index_names()}）")
        if spec.writes_files:
            raise ConfigGateError(f"插件 {name} 会写文件，索引家不允许")

    # 写盘开关：逐条对表，缺键也算错（"默认值恰好对"不是理由）
    import_sec = doc.get("import")
    if not isinstance(import_sec, dict):
        raise ConfigGateError("缺少 import 段（索引家必须显式关掉写盘）")
    for key, want in INDEX_IMPORT_KEYS:
        got = import_sec.get(key)
        if got is None:
            raise ConfigGateError(f"import.{key} 缺失（期望 {want}），不能靠默认值")
        if got != want:
            raise ConfigGateError(f"import.{key} = {got}，索引家要求 {want}")

    # 插件强制参数：xtractor.write 是那个"默认 yes 就写回文件"的键
    for name in names:
        spec = find_index_plugin(name)
        if spec is None or not spec.forced:
            continue
        section = doc.get(name)
        if not isinstance(section, dict):
            raise ConfigGateError(f"缺少 {name} 段（必须显式写死 {dict(spec.forced)}）")
        for key, want in spec.forced:
            got = section.get(key)
            if got is None:
                raise ConfigGateError(f"{name}.{key} 缺失（期望 {want}）")
            if got != want:
                raise ConfigGateError(f"{name}.{key} = {got}，索引家要求 {want}")


def _index_names() -> str:
    return " / ".join(spec.name for spec in INDEX_PLUGINS)


def ensure_config(home: Path | None = None, prof: str | None = None) -> Path:
    """确保 config.yaml 存在且合规，返回它的路径。

    * `workbench`：不存在才写（用户的家，写一次就不再碰）；
    * `index`：每次按模板重渲染（内容相同则不落盘），落盘后过自检。用户没有
      编辑入口（拍板 D），这一侧"手改"只可能是外部污染，重写才是正确反应。

    写入失败抛 OSError（调用方决定是警告还是致命）。
    """
    base = home or data_home()
    prof = prof or profile()
    base.mkdir(parents=True, exist_ok=True)
    path = base / "config.yaml"

    if prof == PROFILE_INDEX:
        # 占位根要真的存在：它代表"整理目标是这个空文件夹"，不存在的话
        # 任何走 destination() 的路径（move / copy 的预演）都会现造一个，
        # 那时"空目录"这个前提就已经被破坏了。
        scratch_dir(base).mkdir(parents=True, exist_ok=True)
        text = render_config(base, None, None, PROFILE_INDEX)
        if not path.is_file() or path.read_text(encoding="utf-8") != text:
            path.write_text(text, encoding="utf-8", newline="\n")
        # 闸对**落盘结果**做，不对内存里的字符串做：盘上的才是 beets 要读的
        verify_config(path.read_text(encoding="utf-8"), PROFILE_INDEX, base)
        return path

    if not path.is_file():
        path.write_text(render_config(base, scan_root()), encoding="utf-8", newline="\n")
    return path
