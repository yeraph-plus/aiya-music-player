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

    args = list(sys.argv[1:] if argv is None else argv)
    # 无参数 = serve（播放器固定一条命令）；有参数时不猜，交给 argparse
    if not args:
        args = ["serve"]

    ap = argparse.ArgumentParser(
        prog="aiya-beets",
        description="AIYA Music 的 beets 侧工具（无子命令时等同于 serve）",
    )
    ap.add_argument("--version", action="version", version=f"aiya-beets {__version__}")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("serve", help="stdio NDJSON 服务（播放器用这个）")
    sub.add_parser("status", help="打印环境体检 JSON")
    sub.add_parser("capabilities", help="打印插件能力三态 JSON")
    sub.add_parser("doctor", help="人类可读的体检报告")
    sub.add_parser("version", help="打印版本号")

    p_install = sub.add_parser("install", help="建 venv 并安装 beets（需要本机 Python）")
    p_install.add_argument("--target", required=True)
    p_install.add_argument("--from-wheels", default=None)
    p_install.add_argument("--requirements", default=None)
    p_install.add_argument("--plugins", default=None, help="要启用的插件名，逗号分隔；all = 全部")
    p_install.add_argument("--dry-run", action="store_true", help="只打印安装计划")

    p_uninstall = sub.add_parser("uninstall", help="删除安装器建的环境（数据不动）")
    p_uninstall.add_argument("--target", required=True)
    p_uninstall.add_argument("--dry-run", action="store_true")

    ns = ap.parse_args(args)

    if ns.cmd == "version":
        print(__version__)
        return 0

    if ns.cmd == "install":
        from . import install

        argv = ["--target", ns.target]
        if ns.from_wheels:
            argv += ["--from-wheels", ns.from_wheels]
        if ns.requirements:
            argv += ["--requirements", ns.requirements]
        if ns.plugins:
            argv += ["--plugins", ns.plugins]
        if ns.dry_run:
            argv.append("--dry-run")
        return install.main(argv)

    if ns.cmd == "uninstall":
        from . import uninstall

        argv = ["--target", ns.target]
        if ns.dry_run:
            argv.append("--dry-run")
        return uninstall.main(argv)

    # 读配置的命令必须在 import beets **之前**把 config.yaml 备好：beets 一导入
    # 就把配置读进内存，之后写的文件对它不生效。以前这一步藏在 serve() 里，
    # 于是 `status` 对新家（还没 config）报的是 beets 默认家的库路径——双家隔离
    # 一上来就报错位。现在统一在这里做，所有读命令看到的是同一份事实。
    try:
        cfg.ensure_config(None, prof)
    except OSError as e:
        # 写不了配置不该让整个应用不可用：beets 用内置默认也能跑，只是插件全关
        print(f"[aiya-beets] 生成配置失败：{e}", file=sys.stderr)
    except cfg.ConfigGateError as e:
        return _config_refusal(e)

    # serve 必须**先于**任何"能不能用"的检查：它的准备动作都在上面做完了。
    if ns.cmd == "serve":
        from .server import serve

        return serve()

    # 其余命令都要能读 beets；不可用时给出可读结论而不是 traceback
    from . import env

    if not env.is_available():
        print(f"beets 不可用（这个解释器：{sys.executable}）", file=sys.stderr)
        print("安装：aiya-beets install --target <dir>，或 pip install beets", file=sys.stderr)
        return 1

    if ns.cmd == "status":
        print(json.dumps(env.status_payload(), ensure_ascii=False))
        return 0

    if ns.cmd == "capabilities":
        print(json.dumps(env.capabilities_payload(), ensure_ascii=False))
        return 0

    if ns.cmd == "doctor":
        return _doctor()

    return 2


def _doctor() -> int:
    """人肉体检。刻意不用 JSON —— 用户贴给我们的应该是这段话，不是一行 JSON。"""
    from . import env
    from .protocol import PROTOCOL_VERSION

    data = env.status_payload()
    print(f"aiya-beets {__version__}  协议 v{PROTOCOL_VERSION}")
    print(f"  Python        {data['pythonVersion']}"
          + ("" if data.get("pythonSupported") else "  ← 不在支持窗口（3.10–3.14）"))
    print(f"  beets         {data['beetsVersion']}")
    print(f"  配置目录      {data['configDir']}")
    print(f"  数据目录      {data['dataHome']}")
    print(f"  库文件        {data['libraryPath']}"
          + ("" if data["libraryExists"] else "  ← 还不存在（首次导入时创建）"))
    root = data.get("libraryDirectory")
    print(f"  整理目标根    {root or '（未配置：整理文件会被拒绝）'}")
    print("  插件：")
    for plug in data["pluginHealth"]:
        mark = "开" if plug["enabled"] else "关"
        line = f"    [{plug['state']}] {plug['name']}"
        if plug["missing"]:
            line += f"   缺：{', '.join(plug['missing'])}"
        if plug["note"]:
            line += f"   — {plug['note']}"
        print(f"{line}   ({mark})")
    counts = data["pluginSummary"]
    print(f"  合计：ready {counts.get('ready', 0)} / "
          f"missing_deps {counts.get('missing_deps', 0)} / "
          f"unavailable {counts.get('unavailable', 0)}")
    return 0
