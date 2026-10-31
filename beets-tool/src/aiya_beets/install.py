"""包内环境管理器：在便携包自己的解释器里改插件与配置。

## 它的形态（2026-11-01 单轨化）

应用只分发便携包。本脚本**随包分发**（就在 aiya_beets 包里），由宿主操作桥
用包内解释器 spawn：

    python -m aiya_beets install --plugins a,b [--previous x,y]

它管的是自己脚下这个环境：pip 装/卸做差量、按 plugins.json 补外部工具、
同步 workbench 配置的 plugins 行、import 自检。安装页的「换插件」勾选确认
后走的就是这一条——原先做了一半的"插件市场"由此闭环。

曾经的"A 轨安装器"形态（用本机 Python 建 venv / --target / --from-wheels /
pyz 物化依赖清单）随单轨化退役——应用不再探测、不再构造、不再使用包外的
任何 Python 环境。见 docs/58 §5 与 git 历史。

## 差量口径

- 目标集 = `--plugins`；装什么包、要哪些外部工具由 `data/plugins.json` 决定
  （脚本不维护第二份依赖表，未知名字直接报错）
- 前一集 = `--previous`（宿主在改写 settings.json 之前传进来的旧启用清单）
- 安装 = 目标集全集（pip 幂等，已满足的要求秒过）
- 卸载 = 前一集 − 目标集（其它插件仍共享的包不会被误删；beets 本体两集都
  不含，永远不动）

## 帧

行帧 JSON 与协议同风格（progress / error / done），宿主逐行解析转成进度事件；
人工在包目录里跑也能直接读。
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

from . import bins, cfg, plugins

# 从 GUI 子系统经 CREATE_NO_WINDOW 拉起的进程自己没有控制台，而 pip 是控制台
# 程序——不补这个参数，每次 pip 都会给用户弹一个黑窗（POSIX 无此参数，空 dict）。
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def parse_plugins(raw: str | None) -> list[str]:
    """`--plugins a,b` → 名字清单（去重保序，空串忽略）。`all` = 表内全部插件。"""
    items = [x.strip() for x in (raw or "").split(",")]
    names = [x for x in items if x]
    if names == ["all"]:
        return [i["name"] for i in plugins.entries()]
    seen: list[str] = []
    for n in names:
        if n not in seen:
            seen.append(n)
    return seen


def _pip(args: list[str], say) -> int:
    """用包内解释器跑 pip，stdout 逐行透传为进度帧。返回 pip 退出码。"""
    cmd = [sys.executable, "-m", "pip", "--disable-pip-version-check", *args]
    with subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
        **_NO_WINDOW,
    ) as proc:
        assert proc.stdout is not None
        for line in proc.stdout:
            line = line.rstrip()
            if line:
                say(line)
        return proc.wait()


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aiya_beets install",
        description="在便携包内变更插件与配置（用包自带解释器运行）",
    )
    ap.add_argument("--plugins", required=True,
                    help="目标插件名，逗号分隔；all = 表内全部")
    ap.add_argument("--previous", default=None,
                    help="变更前的启用清单（宿主传，用于差量卸载）")
    ap.add_argument("--dry-run", action="store_true", help="只打印变更计划，不动环境")
    args = ap.parse_args(argv)

    names = parse_plugins(args.plugins)
    prev = parse_plugins(args.previous)
    try:
        want_pkgs = plugins.packages_for(names)
        prev_pkgs = plugins.packages_for(prev)
        mods = plugins.imports_for(names)
        need_bins = plugins.binaries_for(names)
    except KeyError as e:
        known = ", ".join(s.name for s in plugins.MARKET_CATALOG)
        emit({"type": "error", "stage": "prepare",
              "message": f"不认识的插件 {e}（可用：{known}）"})
        return 2

    # 差量：目标集 − 前一集之外的包要卸。共享依赖天然安全（目标集里还有它就
    # 不会进这个集合）；beets 本体不在任何一集，永远不动。
    drop_pkgs = sorted(set(prev_pkgs) - set(want_pkgs))
    plan = {"plugins": names, "installPackages": want_pkgs,
            "uninstallPackages": drop_pkgs, "checkModules": mods, "binaries": need_bins}
    if args.dry_run:
        emit({"type": "dryRun", "stage": "prepare", **plan})
        emit({"type": "done", "stage": "done", "dryRun": True, **plan})
        return 0

    def say(m: str) -> None:
        emit({"type": "progress", "stage": "pip", "message": m})

    # 1) 差量卸载（先卸后装，腾出被替换的包）。失败不阻断：残留一个"装而未用"
    #    的包无害，不能让它挡住下面的安装。
    if drop_pkgs:
        say(f"pip uninstall {', '.join(drop_pkgs)}")
        _pip(["uninstall", "-y", *drop_pkgs], say)

    # 2) 安装目标集全集
    emit({"type": "progress", "stage": "pip", "message": "pip install 开始"})
    code = _pip(["install", *want_pkgs], say) if want_pkgs else 0
    if code != 0:
        emit({"type": "error", "stage": "pip", "message": f"pip 退出码 {code}"})
        return 5

    # 3) 自检：真 import 一次 beets。只看 pip 退出码不够——装到一半的依赖也能
    #    返回 0。
    check = subprocess.run(
        [sys.executable, "-c", "import beets; print(beets.__version__)"],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        **_NO_WINDOW,
    )
    if check.returncode != 0:
        emit({"type": "error", "stage": "verify", "message": check.stderr.strip()[:500]})
        return 6

    # 4) 插件 import 自检：勾了 lyrics 却 import 不到 bs4，等于白装
    missing: list[str] = []
    for mod in mods:
        got = subprocess.run(
            [sys.executable, "-c", f"import {mod}"],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            **_NO_WINDOW,
        )
        if got.returncode != 0:
            missing.append(mod)
    if missing:
        emit({"type": "error", "stage": "verify",
              "message": f"装完仍缺模块：{', '.join(missing)}（插件要的依赖没装上）"})
        return 6

    # 5) 外部工具：新勾的插件要的 exe 按需下载（ffmpeg 等），落
    #    {data_root}/beets-tool/bin（下载落点优先于包内自带，bin_dirs 决定）。
    #    失败不回滚已装的包——"插件可用、工具没齐"可以点一次重试补齐。
    bin_rows: list[dict] = []
    if need_bins:
        emit({"type": "progress", "stage": "bin",
              "message": f"外置工具：{', '.join(need_bins)}"})
        try:
            bin_rows = bins.ensure(
                need_bins,
                lambda m: emit({"type": "progress", "stage": "bin", "message": m}),
            )
        except (bins.BinError, OSError) as e:
            emit({"type": "error", "stage": "bin",
                  "message": f"插件包已装好，但外置工具没下下来：{e}"})
            return 7

    # 6) 同步 workbench 配置的 plugins 行：宿主已把新清单写进 settings.json，
    #    这里跑一次 ensure 让 config.yaml 跟上（含 _drop_unusable 剔除与门禁）。
    #    同步失败不算全盘失败——下一次会话启动还会再同步。
    try:
        cfg.ensure_config()
    except Exception as e:  # noqa: BLE001 —— 安装成功与配置同步是两件事
        emit({"type": "warning", "message": f"插件已装好，但配置同步失败：{e}"})

    emit({
        "type": "done", "stage": "done",
        "beetsVersion": check.stdout.strip(),
        "installedPlugins": names,
        "uninstalledPackages": drop_pkgs,
        "binaries": bin_rows,
    })
    return 0
