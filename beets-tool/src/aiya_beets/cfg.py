"""配置生成：`config.yaml` 长什么样、放在哪，全归这里。

## 为什么这属于工具侧（docs/33 §2.1）

YAML 键名（`library` / `directory` / `plugins`）、单引号转义规则、哪个键可以省——
这些都是 beets 的知识。播放器不该知道，否则 beets 改配置格式就得跟着改播放器。
现在播放器只提供两个**事实**，通过环境变量传进来：

    AIYA_BEETS_HOME       数据根（播放器给 `{app_data}/beets`）
    AIYA_BEETS_SCAN_ROOT  用户的第一个扫描根（可选；没给就不写 directory）

## 两个刻意的"不"

1. **已存在的 config.yaml 一个字都不动。** 用户手改过的配置不能被应用每次启动
   覆盖；要恢复默认就删掉这个文件。
2. **没有扫描根时省略 directory 行。** 瞎猜一个目录比不写更糟——猜错等于把
   用户的文件 move 到他没预期的地方（`config['directory']` 恒有内置默认
   `~/Music`，所以"不写"和"写默认"是两件事）。
"""

from __future__ import annotations

import os
from pathlib import Path

from .plugins import DEFAULT_PLUGINS


def data_home() -> Path:
    """数据根：环境变量优先，否则用 beets 自己的默认配置目录。

    `AIYA_BEETS_HOME` 由播放器给（它的应用数据目录）；`AIYA_BEETS_DIR` 是
    老名字，dev 冒烟脚本还在用，保留兼容 —— 两者都不设时才回落到 beets 默认。
    """
    for key in ("AIYA_BEETS_HOME", "AIYA_BEETS_DIR"):
        val = os.environ.get(key)
        if val:
            return Path(val)
    from beets import config as beets_config

    return Path(str(beets_config.config_dir()))


def apply_env() -> None:
    """把 `AIYA_BEETS_HOME` 翻译成 beets 认的 `BEETSDIR`。

    **必须在任何 `import beets` 之前调用**：beets 在导入时就把 config_dir
    定下来了，之后再改环境变量没有用（实测：只设 AIYA_BEETS_HOME 时，
    config.yaml 写在自定义目录，而库文件仍落在 `%APPDATA%\\beets` —— 两处分裂，
    用户看到的是"配置生效了但库不对"）。

    显式给的 `BEETSDIR` 优先，不覆盖：冒烟脚本与排障要靠它精确指定。
    """
    if os.environ.get("BEETSDIR"):
        return
    for key in ("AIYA_BEETS_HOME", "AIYA_BEETS_DIR"):
        val = os.environ.get(key)
        if val:
            os.environ["BEETSDIR"] = str(Path(val).expanduser())
            return


def scan_root() -> str | None:
    """用户第一个扫描根（播放器传的事实）；空串/空白视为没给。"""
    val = (os.environ.get("AIYA_BEETS_SCAN_ROOT") or "").strip()
    return os.path.expanduser(val) if val else None


def yaml_path(p: str | Path) -> str:
    """YAML 单引号字符串：里面的反斜杠是字面量，Windows 路径不用再转义。

    双引号里 `\\M` 会被当成转义序列——这是这类配置文件最经典的坑。
    """
    return "'" + str(p).replace("'", "''") + "'"


def render_config(home: Path, root: str | None, plugins: list[str] | None = None) -> str:
    """生成 config.yaml 正文（纯函数，可单测）。

    内容刻意只有三行：library（库文件）、可选 directory（整理目标根）、
    plugins（要开的插件）。beets 的其余配置保持它自己的默认。
    """
    names = plugins if plugins is not None else [p.name for p in DEFAULT_PLUGINS]
    lines = [
        "# AIYA Music 生成；改过之后不会再被覆盖（要恢复默认就删掉本文件）",
        f"library: {yaml_path(home / 'library.db')}",
    ]
    if root:
        lines.append(f"directory: {yaml_path(root)}")
    lines.append("plugins: " + " ".join(names))
    return "\n".join(lines) + "\n"


def ensure_config(home: Path | None = None) -> Path:
    """确保 config.yaml 存在，返回它的路径。

    不存在才写；写入失败抛 OSError（调用方决定是警告还是致命）。
    """
    base = home or data_home()
    base.mkdir(parents=True, exist_ok=True)
    path = base / "config.yaml"
    if not path.is_file():
        path.write_text(render_config(base, scan_root()), encoding="utf-8", newline="\n")
    return path
