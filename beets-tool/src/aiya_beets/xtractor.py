"""让 `beets-xtractor` 在 beets 2.14 上真能跑（docs/54 §3）。

## 为什么要打补丁（docs/50 §3 实测）

`beets-xtractor 0.4.2` 的 `command.py::_get_input_path_for_item` 取的是
`item.get("path")` —— beets 2.14 起那是**相对曲库根**的字节串：

    item.path        → b'D:\\\\MUSIC_Player\\\\test-music\\\\x.wav'   # 绝对（属性）
    item.get('path') → b'x.wav'                                    # 相对（插件用的）

于是每一首都是 `FileNotFoundError: Input file(x.wav) not found!`，而插件只往
日志打一行就跳过（外层 `_run_analysis` 把 `FileNotFoundError` 吞掉，退出码 0）。
症状是「插件装好了、跑完了、bpm 一列全空」——最难查的那一类。

那个包 2024 年之后没再发版（0.4.2 是最后一版），所以修法只能是宿主侧
monkeypatch：**不改第三方文件**，装多少份、装到哪都不打架。

## 为什么不自己写一个插件

只要四个字段（bpm / danceability / beats_count / average_loudness），自己写
一百行也能出。但那样"能不能用"就变成我们要长期维护的第二套实现，而
`beets-xtractor` 的取值映射（`config_default.yml` 里的 path/type）本来是对的。
补一行取路径比复刻整个插件便宜。

## 为什么必须配 `highlevel.compute: 0`（cfg.XTRACTOR_PROFILE）

我们下发的是 AcousticBrainz 那版 **win-i686** 提取器，它没编 Gaia2：配了
svm_models 会 `Identifier 'GaiaTransform' not found in registry` 然后**退出 1、
一个 json 都不产出**（docs/50 §2.5）。所以档位在配置里钉死，这里再兜一次——
`run()` 会先自检那两条键，缺了就直接报可读错误而不是让用户看一天空表。
"""

from __future__ import annotations

import os
import sys

#: 补丁是否已经打上（进程内幂等；插件没装时保持 False）
_PATCHED = False
#: 上一次 `apply()` 失败的原因（给 doctor / 错误消息用）
REASON: str | None = None


class XtractorError(Exception):
    """跑不起来（没装插件 / 没配提取器 / 不是索引家）。消息直接给用户看。"""


def _input_path(self, item) -> str:
    """用 `library.directory` 拼回绝对路径，而不是直接用 `item.path`。

    ## 实测订正（docs/50 §3 记错了一半）

    docs/50 当时写的是"`item.path` 是绝对、`item.get('path')` 是相对"。本轮实测
    （beets 2.14.1，索引家 `directory` 指向曲库根）：**`item.path` 也是相对**——

        item.path        → b'中音测试 440Hz.wav'
        lib.directory    → 'D:\\MUSIC_Player\\test-music'
        拼起来            → b'D:\\MUSIC_Player\\test-music\\中音测试 440Hz.wav'

    所以补丁不能只是"把 `item.get('path')` 换成 `item.path`"（那样照样
    FileNotFoundError），必须走 `beetslib.item_abs_path` —— 那是全仓唯一的
    "相对基准"口径，和查询、导入预览用的是同一个。

    签名必须带 `self`：这个方法被挂在 `XtractorCommand` 上，调用点是
    `self._get_input_path_for_item(item)`（绑方法）。少一个形参就是每首一条
    `TypeError`，而插件只把它算作"这首失败"。

    文件不在就抛 `FileNotFoundError`，让它的 `_run_analysis` 走原本的
    "跳过这一首"分支，而不是我们另造一套错误流。
    """
    from .beetslib import item_abs_path

    lib = getattr(self, "lib", None)
    path = item_abs_path(lib, item) if lib is not None else str(item.path)
    if not os.path.isfile(path):
        raise FileNotFoundError(f"Input file({path}) not found!")
    return path


def apply() -> bool:
    """修掉相对路径 bug。返回是否打上（`beets-xtractor` 没装 → False）。"""
    global _PATCHED, REASON
    if _PATCHED:
        return True
    try:
        from beetsplug.xtractor import command as xt_command
    except Exception as e:  # noqa: BLE001 —— 导入失败的原因不重要，重要的是"没装"
        REASON = f"beets-xtractor 没装（{type(e).__name__}: {e}）"
        return False

    cls = xt_command.XtractorCommand
    if getattr(cls, "_aiya_input_path_patch", False):
        _PATCHED = True
        return True
    cls._get_input_path_for_item = _input_path
    cls._aiya_input_path_patch = True
    _PATCHED = True
    REASON = None
    return True


def readiness() -> dict:
    """三条前提各自的状态（doctor 与 UI 排障用）。"""
    from . import cfg, plugins

    exe = plugins.exe_path("streaming_extractor_music")
    return {
        "plugin": apply(),
        "pluginReason": REASON,
        "extractor": exe,
        "profile": cfg.profile(),
        "profileReady": cfg.profile() == cfg.PROFILE_INDEX,
    }


def run(argv: list[str]) -> int:
    """进程内跑一次 `beet xtractor`（补丁只在同进程生效，所以不能 spawn）。

    为什么强制索引家：`xtractor` 只在索引家的模板里启用、`essentia_extractor`
    也只在那儿写。在工作台家里跑等于拿用户的真库做试验台，而且那一步会
    `item.try_write()` 把 bpm 写进音频文件（工作台家 `write` 默认 yes）——
    那是索引家绝不允许的事。
    """
    from . import cfg

    if cfg.profile() != cfg.PROFILE_INDEX:
        raise XtractorError(
            "xtractor 只在索引家（AIYA_BEETS_PROFILE=index）里跑："
            "工作台家的 write 默认是 yes，会把 bpm 写回用户的音频文件"
        )
    if not apply():
        raise XtractorError(
            f"{REASON}。装法：aiya-beets install --target … --plugins xtractor"
            "（免配置包已预置这一项）"
        )

    # 提取器必须找得到：插件自己的默认值是个占位符，spawn 一个不存在的程序
    # 只会得到一堆 "Value error"，而那是每首一次、上千行的日志。
    from . import plugins

    if plugins.exe_path("streaming_extractor_music") is None:
        raise XtractorError(
            "没有 Essentia 提取器（streaming_extractor_music）：先跑 aiya-beets bins --only "
            "streaming_extractor_music，或者用带它的免配置包"
        )

    from beets import ui

    try:
        ui.main(["xtractor", *argv])
    except SystemExit as e:  # beets 用退出码表达"命令失败"
        return int(e.code or 0)
    return 0


def main(argv: list[str] | None = None) -> int:
    """`aiya-beets xtractor [查询…] [--force] [--threads N]` 的入口。

    `argv` 是 `xtractor` 之后的**原样**参数（cli 用 REMAINDER 收，不再解析一遍）：
    beets 那条命令自己有 `-f/-t/-c/-q/-w/-n` 六个开关，在这里抄一份必然漂。
    """
    from .runtime import force_utf8_stdio

    force_utf8_stdio()
    try:
        return run(list(argv or []))
    except XtractorError as e:
        print(f"[aiya-beets] {e}", file=sys.stderr)
        return 3
