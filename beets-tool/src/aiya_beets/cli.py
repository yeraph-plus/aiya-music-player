"""命令行入口。

约定：**无参数 = serve**。这样播放器可以固定用一条命令起进程
（`aiya-beets` 或 `python -m aiya_beets`），不必在不同场景下换子命令。

其余子命令是给人用的：
    status        一行 JSON（脚本/宿主可解析）
    capabilities  一行 JSON（插件三态）
    doctor        人肉体检（可读文本，出问题不用翻应用日志）
    install       A 轨道安装器（建 venv 装 beets）
    version       版本号
"""

from __future__ import annotations

import argparse
import json
import sys

from . import __version__


def _config_refusal(err: Exception) -> int:
    """环境自相矛盾时的启动应答：一条错误信封，然后退出。

    为什么不用 traceback：这句话要显示给用户（"你把 BEETSDIR 和
    AIYA_BEETS_HOME 指到了两个地方"），而宿主只读得懂 NDJSON。id=0 与
    `_not_installed` 同款——那不是任何请求的应答，是"进程还没开始干活"。
    """
    from .protocol import E_CONFIG, error_response
    from .runtime import force_utf8_stdio, write_line

    force_utf8_stdio()
    write_line(error_response(0, E_CONFIG, "startup", str(err)))
    return 2


def main(argv: list[str] | None = None) -> int:
    # 数据目录必须在任何 import beets 之前翻译成 BEETSDIR（见 cfg.apply_env）
    from . import cfg

    try:
        cfg.apply_env()
        prof = cfg.profile()
    except cfg.ConfigConflict as e:
        return _config_refusal(e)

    # 外部二进制（ffmpeg / Essentia 提取器 / fpcalc）先进 PATH。
    # 必须在任何 import beets 之前：replaygain / convert 用裸名字 "ffmpeg" 去
    # spawn，只有 PATH 一条路；晚一步就是"装了却找不到"。
    from . import bins

    # linux：免配置包 bin/ 前插进 LD_LIBRARY_PATH（提取器的 Qt5 动态依赖，
    # 子进程继承）。挂在 bins 导入之后、PATH 注入之前：bin 目录从数据根推导。
    bins.ensure_loader_path()

    bins.inject_path()

    args = list(sys.argv[1:] if argv is None else argv)
    # 无参数 = serve（播放器固定一条命令）；有参数时不猜，交给 argparse
    if not args:
        args = ["serve"]

    # `xtractor` 之后的参数**原样透传**（beets 那条命令自己有 `-f/-t/-c/-q/-w/-n`）。
    # 为什么在这里分而不是用 `nargs=REMAINDER`：REMAINDER 遇到以 `-` 开头的参数
    # 会被顶层解析器抢走（实测 `aiya-beets xtractor -q` → `unrecognized arguments: -q`）。
    # 只把这一段摘下来，其余子命令仍然走严格解析（`bins --onl x` 该报错还是报错）。
    passthrough: list[str] = []
    if args[0] == "xtractor":
        passthrough, args = args[1:], ["xtractor"]

    ap = argparse.ArgumentParser(
        prog="aiya-beets",
        description="beets-side tool for AIYA Music (no subcommand means serve)",
    )
    ap.add_argument("--version", action="version", version=f"aiya-beets {__version__}")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("serve", help="stdio NDJSON server (what the player uses)")
    sub.add_parser("status", help="print the environment check JSON")
    sub.add_parser("capabilities", help="print the plugin three-state JSON")
    sub.add_parser("doctor", help="human-readable health report")
    sub.add_parser("version", help="print the version")

    p_install = sub.add_parser("install", help="change plugins and config inside the portable bundle (runs on the bundled interpreter)")
    p_install.add_argument("--plugins", required=True,
                           help="target plugin names, comma-separated; all = everything in the table")
    p_install.add_argument("--previous", default=None,
                           help="enabled list before the change (sent by the host, drives differential uninstall)")
    p_install.add_argument("--dry-run", action="store_true", help="print the change plan only")

    p_uninstall = sub.add_parser("uninstall", help="remove installed artifacts (data untouched)")
    p_uninstall.add_argument("--target", required=True)
    p_uninstall.add_argument(
        "--all", action="store_true",
        help="target is the whole beets-tool layer (venv / bundle / bin), not just the venv",
    )
    p_uninstall.add_argument("--dry-run", action="store_true")

    p_bins = sub.add_parser("bins", help="download/verify external binaries into beets-tool/bin")
    p_bins.add_argument("--only", default=None, help="only these names, comma-separated")
    p_bins.add_argument(
        "--from-plugins", default=None, metavar="NAMES",
        help="derive the needed tools from a plugin list (what the host uses)",
    )
    p_bins.add_argument("--check", action="store_true", help="check only, do not download")
    p_bins.add_argument("--dry-run", action="store_true", help="print what would be downloaded")
    p_bins.add_argument(
        "--carrier", action="append", default=[], metavar="NAME=PATH",
        help="use a local package instead of downloading (repeatable)",
    )

    # 其余参数原样交给 `beet xtractor`：那条命令自己有六个开关，抄一份必然漂
    p_xt = sub.add_parser(
        "xtractor",
        help="compute bpm/danceability (index profile; runs beet xtractor in-process, see docs/54)",
    )

    ns = ap.parse_args(args)

    if ns.cmd == "version":
        print(__version__)
        return 0

    if ns.cmd == "install":
        from . import install

        argv = ["--plugins", ns.plugins]
        if ns.previous:
            argv += ["--previous", ns.previous]
        if ns.dry_run:
            argv.append("--dry-run")
        return install.main(argv)

    if ns.cmd == "uninstall":
        from . import uninstall

        argv = ["--target", ns.target]
        if ns.all:
            argv.append("--all")
        if ns.dry_run:
            argv.append("--dry-run")
        return uninstall.main(argv)

    if ns.cmd == "bins":
        argv = []
        if ns.only:
            argv += ["--only", ns.only]
        if ns.from_plugins is not None:
            argv += ["--from-plugins", ns.from_plugins]
        if ns.check:
            argv.append("--check")
        if ns.dry_run:
            argv.append("--dry-run")
        for item in ns.carrier:
            argv += ["--carrier", item]
        return bins.main(argv)

    # 读配置的命令必须在 import beets **之前**把 config.yaml 备好：beets 一导入就把配置读进内存，之后写的文件对它不生效。以前这一步藏在 serve() 里，于是 `status`
    # 对新家（还没 config）报的是 beets 默认家的库路径——双家隔离一上来就报错位。现在统一在这里做，所有读命令看到的是同一份事实。
    try:
        cfg.ensure_config(None, prof)
    except OSError as e:
        # 写不了配置不该让整个应用不可用：beets 用内置默认也能跑，只是插件全关
        print(f"[aiya-beets] failed to generate config: {e}", file=sys.stderr)
    except cfg.ConfigGateError as e:
        return _config_refusal(e)

    # serve 必须**先于**任何"能不能用"的检查：它的准备动作都在上面做完了。
    if ns.cmd == "serve":
        from .server import serve

        return serve()

    # 其余命令都要能读 beets；不可用时给出可读结论而不是 traceback
    from . import env

    if not env.is_available():
        print(f"beets is not installed in this interpreter: {sys.executable}", file=sys.stderr)
        print("to install: re-fetch the portable bundle (ships with beets), or pip install beets into this interpreter", file=sys.stderr)
        return 1

    if ns.cmd == "status":
        print(json.dumps(env.status_payload(), ensure_ascii=False))
        return 0

    if ns.cmd == "capabilities":
        print(json.dumps(env.capabilities_payload(), ensure_ascii=False))
        return 0

    if ns.cmd == "doctor":
        return _doctor()

    if ns.cmd == "xtractor":
        from . import xtractor as xt

        return xt.main(passthrough)

    return 2


def _doctor() -> int:
    """人肉体检。刻意不用 JSON —— 用户贴给我们的应该是这段话，不是一行 JSON。"""
    from . import env
    from .protocol import PROTOCOL_VERSION

    data = env.status_payload()
    print(f"aiya-beets {__version__}  protocol v{PROTOCOL_VERSION}")
    print(f"  Python        {data['pythonVersion']}"
          + ("" if data.get("pythonSupported") else "  <- outside the supported window (3.10-3.14)"))
    print(f"  beets         {data['beetsVersion']}")
    print(f"  config dir    {data['configDir']}")
    print(f"  data dir      {data['dataHome']}")
    print(f"  library file  {data['libraryPath']}"
          + ("" if data["libraryExists"] else "  <- does not exist yet (created on first import)"))
    root = data.get("libraryDirectory")
    print(f"  organize root {root or '(not configured: organizing files will be rejected)'}")
    print("  plugins:")
    for plug in data["pluginHealth"]:
        mark = "on" if plug["enabled"] else "off"
        line = f"    [{plug['state']}] {plug['name']}"
        if plug["missing"]:
            line += f"   missing: {', '.join(plug['missing'])}"
        if plug["note"]:
            line += f"   - {plug['note']}"
        print(f"{line}   ({mark})")
    counts = data["pluginSummary"]
    print(f"  total: ready {counts.get('ready', 0)} / "
          f"missing_deps {counts.get('missing_deps', 0)} / "
          f"unavailable {counts.get('unavailable', 0)}")
    return 0
