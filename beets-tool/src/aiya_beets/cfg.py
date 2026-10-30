"""配置生成：`config.yaml` 长什么样、放在哪，全归这里。

## 为什么这属于工具侧（docs/33 §2.1）

YAML 键名（`library` / `directory` / `plugins`）、单引号转义规则、哪个键可以省——
这些都是 beets 的知识。播放器不该知道，否则 beets 改配置格式就得跟着改播放器。
现在播放器只提供两个**事实**，通过环境变量传进来：

    AIYA_BEETS_HOME       数据根（播放器给 `{app_data}/beets` 或 `{app_data}/beets-index`）
    AIYA_BEETS_SCAN_ROOT  用户的第一个扫描根（可选；没给就不写 directory）
    AIYA_BEETS_PROFILE    `workbench`（默认，用户库）| `index`（索引库，docs/45 §4.1）

## 两个刻意的"不"

1. **已存在的 config.yaml 里，除了 `directory` 那一行，一个字都不动**（仅 `workbench`
   家）。用户手改过的配置不能被应用每次启动覆盖；要恢复默认就删掉这个文件。
2. **没有扫描根时省略 directory 行**（仅 `workbench` 家）。瞎猜一个目录比不写更糟
   ——猜错等于把用户的文件 move 到他没预期的地方（`config['directory']` 恒有内置
   默认 `~/Music`，所以"不写"和"写默认"是两件事）。

## 为什么 `directory` 是例外（2026-10-30 真机事故）

它**不是用户偏好，是应用事实**：曲库根（`AIYA_BEETS_SCAN_ROOT`）与整理落点
（`AIYA_BEETS_DEST`）由播放器决定，用户没有编辑入口。

原实现把它也归进"写一次就不再碰"，于是：用户第一次进设置页时还没有曲库目录
→ 生成 config 时省略 `directory` → **此后永远不补**。结果 `beets move/copy`
用内置默认 `~/Music`，整理出来的文件全落到 `C:/Users/<me>/Music/Non-Album/…`，
而界面上明明写着落点是 `D:/Music/整理`。实测撞到：两首歌被 copy 到
`~/Music/Non-Album/`，落点目录是空的。

所以 `directory` 单独走 [`_sync_directory`]：只改这一行（没有就插在 `library`
之后），其余行逐字节保留。这也是 docs/49 §7.1「落点外科式修改」那条测试。

## 索引家为什么反过来（docs/45 §6.1）

`index` 家的 config.yaml 是**生成的工件，不是用户配置**：它承载"永不写文件"这条
安全前提（`import.write/copy/move: no` + `xtractor.write: no` + 插件白名单），
而这几条全靠 beets 的默认值恰好不对——xtractor 的 `write` 默认就是 `yes`。
所以这一侧：

* `directory` 指向**曲库根**（docs/49）——索引家是"用户曲库的只读镜像"，分析对象
  就是曲库本身，不再另造一个占位目录。没给曲库根时退回 `{home}/scratch` 占位，
  因为"空目录"是它唯一需要满足的条件；
* 每次启动都按模板重渲染（内容相同则不动盘），**渲染后自检**——断言关键键存在
  且为期望值，不依赖"默认值恰好对"；
* 用户在 UI 里没有编辑入口（拍板 D），要改就改模板。

### 一条被推翻的旧结论（docs/49 §1.2）

这里原先写着"`directory` 指向扫描根会让库里存相对路径、`path::` 定位失效"，因此
恒用占位目录。beets 2.14.1 实测：**磁盘上确实存相对**（`Album/x.flac`），但
`Item.path` 这个 API 读出来是绝对（beets 用 `config.directory` 拼回去了），
`PathQuery` 相对/绝对两种口径都能命中。所以只要走 Python API（本工具全部走 API），
指向曲库根是安全的。兜底仍然留着：查询出口把绝对路径再 `join` 一次（幂等）。

代价是"口径变了"：老索引库的记录是绝对路径、新记录是相对。用户拍板**干净重建**，
所以 `ensure_config` 用 `layout.json` 记下当前口径，变了就成对删库与增量状态。

自检的意义不在"模板写对了没有"（那是同义反复），而在于**任意一份落盘的文本**
都必须过闸：老版本写的、手改的、被别的工具动过的，过不去就重写。测试里的负向
验证正是往文本里改一个 `write: yes`，闸必须变红。
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

from .plugins import (
    DEFAULT_PLUGINS,
    INDEX_DEFAULT_PLUGINS,
    INDEX_PLUGINS,
    exe_path,
    find_index_plugin,
    usable_plugins,
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

# xtractor 的提取档位（docs/54 §3）。**必须钉成"不算 highlevel"**：
# 我们下发的是 AcousticBrainz 那版 **win-i686** 提取器，它没编 Gaia2 ——
# 配了 svm_models 会 `Identifier 'GaiaTransform' not found in registry`
# 然后**退出 1 且一个 json 都不产出**（docs/50 §2.5 实测）。插件自己那份
# `config_default.yml` 默认 `highlevel.compute: 1` + 一个占位模型路径，
# 所以不钉就永远是"跑完了、字段全空"。
XTRACTOR_PROFILE: dict[str, object] = {
    "outputFormat": "json",
    "highlevel": {"compute": 0},
}


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
    """**曲库根**（播放器传的事实）：索引家拿它当分析对象，与落点无关。

    索引家曾经与整理家共用这个变量，结果"改落点"会把索引范围一起改掉
    （落点常常只是曲库里一个子目录）。docs/49 §3.2 把两者拆开：这个键永远
    只表示曲库根，落点走 `dest_root()`。
    """
    val = (os.environ.get("AIYA_BEETS_SCAN_ROOT") or "").strip()
    return os.path.expanduser(val) if val else None


def dest_root() -> str | None:
    """**整理落点**：copy/move 的目标根（整理家写进 `directory`）。

    没给就回退曲库根——那是"整理输出"这个功能出现之前的行为，老配置零迁移。
    """
    val = (os.environ.get("AIYA_BEETS_DEST") or "").strip()
    return os.path.expanduser(val) if val else None


def yaml_path(p: str | Path) -> str:
    """YAML 单引号字符串：里面的反斜杠是字面量，Windows 路径不用再转义。

    双引号里 `\\M` 会被当成转义序列——这是这类配置文件最经典的坑。
    """
    return "'" + str(p).replace("'", "''") + "'"


def scratch_dir(home: Path) -> Path:
    """索引家的占位整理根（空目录，永不承载文件）。

    只在"没给曲库根"时用作 `directory`：beets 需要一个存在的目录，而索引家
    永不做 move/copy，所以这个值只要存在、且不是曲库根就够。
    """
    return home / SCRATCH_DIR


def index_dir(home: Path, root: str | None) -> Path:
    """索引家的 `directory`：曲库根（只读分析对象），没给根时退回占位目录。

    为什么不再恒用占位目录（docs/49 §1.2 的实测）：索引家是"曲库的只读镜像"，
    指到曲库根之后 beets 的 `path` 语义与用户的真实目录一致，查询结果拿去
    `tracks.path_lower` 关联才对得上，不用先翻译一次。
    """
    return Path(root).expanduser() if root else scratch_dir(home)


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
    （docs/45 §6.1）。`root` 在这一侧是**曲库根**（只读分析对象，docs/49）；
    没给时退回占位目录。
    """
    if profile not in PROFILES:
        raise ConfigConflict(f"未知 profile: {profile!r}")
    if profile == PROFILE_INDEX:
        return _render_index(home, plugins, root)
    return _render_workbench(home, root, plugins)


def _render_workbench(home: Path, root: str | None, plugins: list[str] | None) -> str:
    if plugins is not None:
        names = list(plugins)
    else:
        # 用户在安装页勾过的清单优先（装了不启用等于白装）；
        # 没勾过才用表里的默认清单
        names = settings_plugins(home) or [p.name for p in DEFAULT_PLUGINS]
    names = _drop_unusable(names, "workbench")
    lines = [
        "# AIYA Music 生成；改过之后不会再被覆盖（要恢复默认就删掉本文件）",
        f"library: {yaml_path(home / 'library.db')}",
    ]
    if root:
        lines.append(f"directory: {yaml_path(root)}")
    lines.append("plugins: " + " ".join(names) if names else "plugins: []")
    return "\n".join(lines) + "\n"



def _drop_unusable(names: list[str], where: str) -> list[str]:
    """剔掉"外部依赖还缺"的插件，并在 stderr 说明剔了谁（docs/53 §1.1）。

    为什么必须剔：`plugins: replaygain` 而 PATH 上没有 ffmpeg 时，beets 的**任何**
    子命令都起不来（插件在启动时初始化，`UserError` 直接抛）。写了却缺 exe，
    不是"该功能降级"，是"组件不可用"。

    为什么只在这里剔、不改 `settings.json`：用户的选择要留着。等他补上工具
    （`aiya-beets bins`）之后，下一次渲染自动恢复启用——不需要重新勾一遍。
    """
    kept = usable_plugins(names)
    dropped = [n for n in names if n not in kept]
    if dropped:
        # stderr 而不是 stdout：协议帧只走 stdout，这条是给人排查用的
        print(
            f"[aiya-beets] {where}：{', '.join(dropped)} 缺外部工具，本次未启用"
            "（aiya-beets bins 可补齐）",
            file=sys.stderr,
        )
    return kept


SETTINGS_FILE = "settings.json"


def settings_plugins(home: Path | None = None) -> list[str] | None:
    """宿主记在 `settings.json` 里的启用清单（`None` = 用户没选过）。

    为什么读这里而不是再发一个环境变量：`settings.json` 本来就在同一个 home
    下（宿主拥有、工具侧只读）。给同一个事实开第二条通路，两条一旦不一致，
    症状就是"安装页勾了、config 里没启用"——这类静默不一致最难查。
    坏 JSON / 缺键 / 空数组都按"没选过"处理：配置坏了不该让组件整体不可用。
    """
    base = home or data_home()
    try:
        doc = json.loads((base / SETTINGS_FILE).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    raw = doc.get("plugins") if isinstance(doc, dict) else None
    if not isinstance(raw, list):
        return None
    names = [str(x).strip() for x in raw if isinstance(x, str) and x.strip()]
    return names or None


# `directory:` 行（含行尾），用来做外科式替换。故意只认行首：被注释掉的
# `# directory:` 不该被当成"已经有这一行"。
_DIRECTORY_LINE = re.compile(r"^directory:[^\n]*$", re.MULTILINE)


def _sync_directory(text: str, root: str) -> str:
    """把 `directory` 行改成 `root`，**其余行逐字节保留**。

    为什么不整份重渲染：workbench 是用户的家，`plugins`、`path_formats`、
    他自己加的任何行都算他的；整份重写等于每次启动回滚一次他的配置。

    但 `directory` 是例外——它是应用事实（曲库根 / 整理落点），而且必须跟着变，
    否则第一次没配目录时省掉的那一行永远补不回来，`move/copy` 就落到 beets
    内置默认的 `~/Music`（真机事故，见模块头）。
    """
    want = f"directory: {yaml_path(root)}"
    if _DIRECTORY_LINE.search(text):
        # 用函数替换而不是字符串：路径里的反斜杠会被 re.sub 当转义序列吃掉
        return _DIRECTORY_LINE.sub(lambda _m: want, text, count=1)
    # 没有就插在 library 行之后：beets 的键顺序无所谓，但人读起来这两行挨着最自然
    lines = text.splitlines(keepends=True)
    for i, line in enumerate(lines):
        if line.startswith("library:"):
            lines.insert(i + 1, want + "\n")
            return "".join(lines)
    return want + "\n" + text


# `plugins:` 行（含行尾）。同样只认行首：注释掉的 `# plugins:` 不算。
_PLUGINS_LINE = re.compile(r"^plugins:[^\n]*$", re.MULTILINE)


def _sync_plugins(text: str, names: list[str]) -> str:
    """把 `plugins:` 行改成 `names`，**其余行逐字节保留**。

    为什么这是应用事实而不是用户偏好：用户在安装页勾了哪些插件、安装器就把
    哪些依赖装进了 venv。config 里不启用，等于装了个摆设。所以它要跟着
    `settings.json` 走，与 `directory` 同一类处理（外科式替换一行）。

    空清单写成 `plugins: []` 而不是留空：空值是 YAML 的 None，
    beets 的 `as_str_seq` 对它怎么处理不值得赌。
    """
    want = "plugins: " + (" ".join(names) if names else "[]")
    if _PLUGINS_LINE.search(text):
        return _PLUGINS_LINE.sub(lambda _m: want, text, count=1)
    lines = text.splitlines(keepends=True)
    # 插在 library/directory 之后：beets 的键顺序无所谓，人读起来挨着最自然
    for anchor in ("directory:", "library:"):
        for i, line in enumerate(lines):
            if line.startswith(anchor):
                lines.insert(i + 1, want + "\n")
                return "".join(lines)
    return want + "\n" + text


def _render_tree(rows: list[tuple[str, object]], indent: int) -> list[str]:
    """按 4 空格层级渲染 `key: value`（`dict` 值就往下嵌一层）。

    为什么需要层级：xtractor 的 `extractor_profile.highlevel.compute` 是 confuse
    的**真嵌套**键。写成点号扁键（`extractor_profile.highlevel.compute: 0`）是
    查不到的——confuse 把 `config['a.b']` 拆成两层去找，而 YAML 里那是一个
    字面 key（这条在验证脚本里踩过：插件报 `Key 'extractor_profile' is not defined`）。
    """
    out: list[str] = []
    pad = " " * indent
    for key, value in rows:
        if isinstance(value, dict):
            out.append(f"{pad}{key}:")
            out += _render_tree(list(value.items()), indent + 4)
        else:
            out.append(f"{pad}{key}: {value}")
    return out


def _render_index(home: Path, plugins: list[str] | None, root: str | None) -> str:
    names = list(plugins) if plugins is not None else list(INDEX_DEFAULT_PLUGINS)
    # xtractor 要 Essentia 提取器；没有它写进 plugins 就是"假开"——
    # 每次 import 都吐一段 `PluginImportError: Could not import plugin xtractor`
    # 然后照常跑（退出码 0，docs/50 §1.1 实测）。按环境过滤掉才是诚实的做法。
    names = _drop_unusable(names, "index")
    lines = [
        "# AIYA Music 索引库（DB1）：这是生成物，不要手改——改模板（cfg.render_config）。",
        "# 可随时重建：删掉 library.db* 与 state.pickle **成对删**（state 才是增量记忆）。",
        f"library: {yaml_path(home / 'library.db')}",
        f"directory: {yaml_path(index_dir(home, root))}",
        "plugins: " + " ".join(names) if names else "plugins: []",
        "import:",
    ]
    lines += [f"    {key}: {value}" for key, value in INDEX_IMPORT_KEYS]
    # 每个启用插件的强制参数（xtractor 的 write: yes 会把 bpm 写回音频文件）
    for name in names:
        spec = find_index_plugin(name)
        if spec is None:
            continue
        rows: list[tuple[str, object]] = list(spec.forced)
        # xtractor 唯一一个"必须给绝对路径"的键：它 spawn 的外部程序没有默认位置
        # （默认值就是占位符 `/your/path/to/streaming_extractor_music`）。
        if name == "xtractor":
            found = exe_path("streaming_extractor_music")
            if found:
                rows.append(("essentia_extractor", yaml_path(found)))
            rows.append(("extractor_profile", XTRACTOR_PROFILE))
        if not rows:
            continue
        lines.append(f"{name}:")
        lines += _render_tree(rows, 4)
    return "\n".join(lines) + "\n"


def _parse(text: str) -> dict[str, object]:
    """极简 YAML 子集解析：`key: value` 按缩进叠层级（每层 4 空格）。

    为什么自己解析而不是 `yaml.safe_load`：门禁要在**任何**落盘文本上工作，
    包括被人手改坏的；用完整解析器会因为一处语法错误抛异常，而我们要的是
    "这条键不对"这种可读结论。只解析我们自己渲染的那几种形状就够了，
    解析不出来的形状一律视为不合规（拒绝比宽容安全）。

    缩进必须是 4 的倍数：渲染侧只产出这一种（`_render_tree`），别的形状说明
    这份文件不是我们写的，或者被人动过——两种都该拦。
    """
    root: dict[str, object] = {}
    stack: list[tuple[int, dict[str, object]]] = [(-1, root)]
    for raw in text.splitlines():
        line = raw.rstrip()
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        stripped = line.lstrip()
        indent = len(line) - len(stripped)
        if indent % 4:
            raise ConfigGateError(f"缩进不是 4 的倍数（无法识别的配置行）：{raw!r}")
        key, _, value = stripped.partition(":")
        key, value = key.strip(), value.strip()
        if not key:
            raise ConfigGateError(f"无法识别的配置行（缺键名）：{raw!r}")
        while stack and indent <= stack[-1][0]:
            stack.pop()
        if not stack:
            raise ConfigGateError(f"无法识别的配置行（缩进越界）：{raw!r}")
        parent = stack[-1][1]
        if value:
            parent[key] = value
        else:
            child: dict[str, object] = {}
            parent[key] = child
            stack.append((indent, child))
    return root


def _plugin_names(value: object) -> list[str]:
    """`plugins:` 的两种写法都收：`xtractor lastgenre` 与 `[xtractor, lastgenre]`。"""
    if value is None:
        return []
    text = str(value).strip().strip("[]")
    return [part for part in text.replace(",", " ").split() if part]


def verify_config(
    text: str,
    profile: str = PROFILE_INDEX,
    home: Path | None = None,
    root: str | None = None,
) -> None:
    """索引家配置的自检闸。不合规抛 `ConfigGateError`，消息说清哪一条不对。

    `workbench` 家不设闸：那是用户的家，任何写法都合法。

    `root` 是**期望的曲库根**（索引家的 `directory`）。传 None 表示"这一侧应该
    是占位目录"——即宿主没给曲库根的那种形态。闸必须能分辨两者，否则"目录被
    改回 scratch"这种退化会静默通过（docs/49 §7.4 的负向验证正是这一条）。
    """
    if profile != PROFILE_INDEX:
        return
    doc = _parse(text)

    if not doc.get("library"):
        raise ConfigGateError("缺少 library（索引家必须有独立的库文件）")

    # library 不能只判"非空"：写错路径时 beets 会**静默新建一个空库**，
    # 症状是"索引跑完了，统计一直是 0"——而配置看起来完全合规。
    # 期望值是 `{home}/library.db`；home 未知（自检时没传）就只校验目录部分。
    got_lib = _norm(str(doc["library"]).strip("'\""))
    if home is not None:
        want_lib = _norm(str(Path(home) / "library.db"))
        if got_lib != want_lib:
            raise ConfigGateError(
                f"library 必须指向索引家自己的库 {want_lib}，实际是 {got_lib}"
                "（指向别处会让索引写进另一个库，工作清单与查询会各看一个库）"
            )
    elif not got_lib.endswith("library.db"):
        raise ConfigGateError(f"library 必须是一个 .db 文件路径，实际是 {got_lib}")

    # 分析对象：必须是**曲库根**（docs/49）；没给根时才是占位目录。
    # 两种形态都要能分辨：指错时症状是"索引范围悄悄缩成某个子目录"，零报错。
    if not doc.get("directory"):
        raise ConfigGateError("缺少 directory（索引家必须写死分析对象，不能省）")
    got_dir = _norm(str(doc["directory"]).strip("'\""))
    if home is not None:
        want_dir = _norm(index_dir(home, root))
        if got_dir != want_dir:
            raise ConfigGateError(
                f"directory 必须是 {index_dir(home, root)}，实际是 {doc['directory']}"
                "（索引家对着曲库根做只读分析；指到别处等于分析了一批用户没让你看的文件）"
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

    # 外部依赖 (docs/53 §1.1)：写了却缺 exe 的插件会让 beets 任何命令都起不来。
    # 渲染侧已经剔过 —— 这条是"模板与过滤漂了"的兜底，不是主要防线。
    usable = usable_plugins(names)
    stuck = [n for n in names if n not in usable]
    if stuck:
        raise ConfigGateError(
            f"插件 {', '.join(stuck)} 缺外部工具，写进 plugins 会让 beets 起不来"
            "（应先用 aiya-beets bins 补齐，或从模板里剔掉）"
        )

    # xtractor 唯一一个必须给绝对路径的键：spawn 的外部程序没有默认位置
    if "xtractor" in names:
        found = exe_path("streaming_extractor_music")
        section = doc.get("xtractor")
        got = str(section.get("essentia_extractor", "")).strip("'\"") if isinstance(section, dict) else ""
        if found and _norm(got) != _norm(found):
            raise ConfigGateError(
                f"xtractor.essentia_extractor 必须指向 {found}，实际是 {got or '（缺失）'}"
                "（默认值是占位符，插件会拿它去 spawn 一个不存在的程序）"
            )
        # 提取档位：highlevel 必须关掉（docs/54 §3）。开着的话 win-i686 那版
        # 提取器会 `GaiaTransform not found` 退出 1，一个 json 都不产出——
        # 症状是"跑了，字段全空"，而配置看起来完全正常。
        profile = section.get("extractor_profile") if isinstance(section, dict) else None
        high = profile.get("highlevel") if isinstance(profile, dict) else None
        compute = high.get("compute") if isinstance(high, dict) else None
        if compute != "0":
            raise ConfigGateError(
                "xtractor.extractor_profile.highlevel.compute 必须是 0"
                f"（实际 {compute or '（缺失）'}）：我们下发的提取器没编 Gaia2，"
                "算 highlevel 会整体退出 1 且不产出任何结果"
            )


def _index_names() -> str:
    return " / ".join(spec.name for spec in INDEX_PLUGINS)


LAYOUT_FILE = "layout.json"

#: 索引家的库 + 增量状态。**成对删**：只删库会让 `-R` 的记忆仍在，
#: 下次导入"全都导过了"直接跳过，症状是"索引恒空且零报错"（docs/45 实测）。
INDEX_STATE_FILES = ("library.db", "library.db-wal", "library.db-shm", "state.pickle")


def index_layout(home: Path, root: str | None) -> dict[str, str]:
    """索引库的"口径指纹"：只记决定路径语义的那一个值。

    为什么要记：`directory` 换了，库里 `items.path` 的相对基准就换了——旧行是
    绝对、新行是相对，混存的后果不是报错，是"清旧步骤认不出自己写过什么"。
    用户已拍板口径变化就干净重建（docs/49 §3.3），这里负责认出"变了"。
    """
    return {"directory": _norm(index_dir(home, root))}


def _read_layout(path: Path) -> dict[str, str] | None:
    """读口径指纹。不存在/坏文件一律当"没有"——那会被判成变化，触发重建。"""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def reset_index_state(home: Path) -> list[str]:
    """删索引库与增量状态，返回实际删掉的文件名（调用方可据此报告重建）。"""
    removed: list[str] = []
    for name in INDEX_STATE_FILES:
        target = home / name
        if not target.exists():
            continue
        try:
            target.unlink()
            removed.append(name)
        except OSError:
            # 被别的进程占着（比如同时跑着索引）：留着让下一次启动再试，
            # 而不是把整个配置准备变成致命错误。
            continue
    return removed


def ensure_config(home: Path | None = None, prof: str | None = None) -> Path:
    """确保 config.yaml 存在且合规，返回它的路径。

    * `workbench`：不存在才写（用户的家，写一次就不再碰）；
    * `index`：每次按模板重渲染（内容相同则不落盘），落盘后过自检。用户没有
      编辑入口（拍板 D），这一侧"手改"只可能是外部污染，重写才是正确反应。
      口径（`directory`）变化时**先成对删库**再写——旧记录的路径语义已经对不上。

    写入失败抛 OSError（调用方决定是警告还是致命）。
    """
    base = home or data_home()
    prof = prof or profile()
    base.mkdir(parents=True, exist_ok=True)
    path = base / "config.yaml"

    if prof == PROFILE_INDEX:
        root = scan_root()
        if root is None:
            # 只有退回占位目录时才需要造它：它代表"整理目标是这个空文件夹"，
            # 不存在的话任何走 destination() 的路径（move/copy 的预演）都会现造。
            # 曲库根则相反——绝不能替用户建目录（那是用户的音乐文件夹）。
            scratch_dir(base).mkdir(parents=True, exist_ok=True)
        want = index_layout(base, root)
        layout_path = base / LAYOUT_FILE
        if _read_layout(layout_path) != want:
            removed = reset_index_state(base)
            layout_path.write_text(
                json.dumps(want, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
                newline="\n",
            )
            if removed:
                # stderr 而不是 stdout：协议帧只走 stdout，这条是给人排查用的。
                print(
                    "索引库按新口径重建：directory=%s，已删 %s"
                    % (want["directory"], ", ".join(removed)),
                    file=sys.stderr,
                )
        text = render_config(base, root, None, PROFILE_INDEX)
        if not path.is_file() or path.read_text(encoding="utf-8") != text:
            path.write_text(text, encoding="utf-8", newline="\n")
        # 闸对**落盘结果**做，不对内存里的字符串做：盘上的才是 beets 要读的
        verify_config(path.read_text(encoding="utf-8"), PROFILE_INDEX, base, root)
        return path

    if not path.is_file():
        path.write_text(
            render_config(base, dest_root() or scan_root()), encoding="utf-8", newline="\n"
        )
    else:
        # 已存在 → 只同步两行应用事实，其余逐字节保留。
        # 为什么要同步 `directory`：第一次生成时可能还没有曲库目录（那行被省略），
        # 此后用户配上了落点，若永不补，move/copy 会落进 beets 默认的 ~/Music。
        # 为什么要同步 `plugins`：安装页勾选的插件装了依赖却没启用，等于白装。
        # 但**只在用户选过时才同步**（settings.json 里没有这个键就不碰），
        # 否则会每次启动把用户手改的 plugins 行回滚掉。
        text = path.read_text(encoding="utf-8")
        want = text
        root = dest_root() or scan_root()
        if root:
            want = _sync_directory(want, root)
        chosen = settings_plugins(base)
        if chosen:
            # 勾了但外部工具还没下载的，这一版先不启用（否则 beets 起不来，docs/53）。
            # settings.json 不改：等 bins 补齐后下一次启动自动恢复。
            want = _sync_plugins(want, _drop_unusable(chosen, "workbench"))
        if want != text:
            path.write_text(want, encoding="utf-8", newline="\n")
    return path
