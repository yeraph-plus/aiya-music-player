#!/usr/bin/env python
"""构建 A 轨道发布物：`beets-installer.pyz`。

## 为什么用 `.pyz` 而不是 wheel

A 轨道的用户场景是"本机已有 Python，但不想碰 pip"。`.pyz` 是一个可执行的
zip（Python 内置支持），用户/播放器只要 `python beets-installer.pyz install
--target <dir>` 就能装，不需要先 `pip install` 一个引导包。

## 为什么这个脚本是 Python 而不是 Node

`zipapp` 是标准库，一行搞定；Node 侧要为此引入压缩库。打包脚本属于 beets 侧
资产（docs/33 §3.1），用 Python 写更贴近它服务的东西。
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import zipapp
from pathlib import Path

HERE = Path(__file__).resolve().parent          # beets-tool/tools
TOOL = HERE.parent                              # beets-tool
SRC = TOOL / "src"


def _force_utf8_stdio() -> None:
    """把这个脚本自己的 stdout/stderr 切成 UTF-8。

    为什么必须做：Windows 上**管道**里的 stdout 按 locale 编码（英文 runner 是
    cp1252），中文 print 直接 `UnicodeEncodeError`。GitHub Actions 的 release
    工作流就是这么崩的（崩在下面那句 `print(f"OK …")`）。

    为什么不能用 `os.environ.setdefault("PYTHONUTF8", "1")`（以前就是这么写的）：
    那个变量只在**解释器启动前**有效，脚本里设它时 stdio 早就用 locale 编码打开了
    ——看着像修了，实际是空操作。`reconfigure` 改的是已打开流本身。
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass   # 被重定向成非 TextIOWrapper（如管道给别的程序）就算了


def main() -> int:
    ap = argparse.ArgumentParser(description="打包 aiya-beets 安装器（.pyz）")
    ap.add_argument("--out", default=str(TOOL / "dist" / "beets-installer.pyz"))
    args = ap.parse_args()

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)

    # 两条硬要求：依赖清单与许可全文都必须**在包内**。
    #   · requirements.txt：安装器从 data/ 读（见 install.requirements_file）
    #   · LICENSE：GPL-3.0 §4/§6 要求分发目标代码附许可全文。这个 pyz 是
    #     独立发布单元——用户拿到它时仓库不在手边，指着 `../LICENSE` 等于没给。
    #
    # 两者都写在**临时 staging 目录**里，不碰源码树：以前的版本直接往
    # `SRC/aiya_beets/data/` 写，于是每次打包都会在 git 里留一个未跟踪文件
    # （2026-10-27 换成 staging，顺便让 `.pyz` 内容与仓库状态解耦）。
    lock = TOOL / "requirements.lock"
    lic = TOOL / "LICENSE"
    if not lic.is_file():
        print("缺少 LICENSE：GPL 要求发布物内附许可全文", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="aiya-pyz-") as tmp:
        stage = Path(tmp) / "src"
        shutil.copytree(SRC, stage, ignore=shutil.ignore_patterns("__pycache__"))
        data = stage / "aiya_beets" / "data"
        data.mkdir(parents=True, exist_ok=True)
        if lock.is_file():
            shutil.copyfile(lock, data / "requirements.txt")
        shutil.copyfile(lic, data / "LICENSE")
        # 插件表由 copytree 带进来了，但它现在是**必需**的：`plugins.py` 在
        # import 期就读它（派生 MARKET/DEFAULT/INDEX 三个视图）。缺了的话
        # 安装器一启动就 FileNotFoundError，所以宁可在这里硬红一次。
        if not (data / "plugins.json").is_file():
            print("缺少 data/plugins.json：插件表必须随包分发", file=sys.stderr)
            return 1

        zipapp.create_archive(
            stage,
            target=out,
            interpreter="/usr/bin/env python3",
            # 不能直接指 cli:main：zipapp 生成的 __main__.py 会丢掉返回值，
            # 于是 pyz **永远退出 0**（实测：`bins --only nope` 报错但 exit=0）。
            # entry.main 只做 sys.exit(cli_main())。
            main="aiya_beets.entry:main",
            compressed=True,
        )
    size = out.stat().st_size

    # 自检：能跑起来才算打完（否则产物是死的，装到用户机器上才发现）
    # 子进程的 stdio 是管道、编码由它自己决定，所以显式给 UTF-8 环境
    probe = subprocess.run(
        [sys.executable, str(out), "version"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8"},
    )
    if probe.returncode != 0 or not probe.stdout.strip():
        print(f"打包自检失败：{probe.stdout} {probe.stderr}", file=sys.stderr)
        return 1

    # 退出码自检：错误路径必须**非零**退。以前 zipapp 的 `main=` 会丢掉返回值，
    # 于是 pyz 永远 exit 0 —— 上层看退出码就以为装好了（"静默成功"这类事故
    # 一旦进到发布物里，用户侧没有任何线索）。
    bad = subprocess.run(
        [sys.executable, str(out), "bins", "--only", "nope"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8"},
    )
    if bad.returncode == 0:
        print(
            "打包自检失败：错误路径退出码是 0（pyz 入口丢了返回值？）",
            file=sys.stderr,
        )
        return 1

    print(f"OK  {out}  {size / 1024:.0f} KB  自检版本 {probe.stdout.strip()}"
          f"  错误路径退出码 {bad.returncode}")
    return 0


if __name__ == "__main__":
    _force_utf8_stdio()
    sys.exit(main())
