#!/usr/bin/env python
"""构建 A 轨道发布物：`aiya-beets-installer.pyz`。

## 为什么用 `.pyz` 而不是 wheel

A 轨道的用户场景是"本机已有 Python，但不想碰 pip"。`.pyz` 是一个可执行的
zip（Python 内置支持），用户/播放器只要 `python aiya-beets-installer.pyz install
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


def main() -> int:
    ap = argparse.ArgumentParser(description="打包 aiya-beets 安装器（.pyz）")
    ap.add_argument("--out", default=str(TOOL / "dist" / "aiya-beets-installer.pyz"))
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

        zipapp.create_archive(
            stage,
            target=out,
            interpreter="/usr/bin/env python3",
            main="aiya_beets.cli:main",
            compressed=True,
        )
    size = out.stat().st_size

    # 自检：能跑起来才算打完（否则产物是死的，装到用户机器上才发现）
    probe = subprocess.run(
        [sys.executable, str(out), "version"], capture_output=True, text=True, encoding="utf-8"
    )
    if probe.returncode != 0 or not probe.stdout.strip():
        print(f"打包自检失败：{probe.stdout} {probe.stderr}", file=sys.stderr)
        return 1

    print(f"OK  {out}  {size / 1024:.0f} KB  自检版本 {probe.stdout.strip()}")
    return 0


if __name__ == "__main__":
    os.environ.setdefault("PYTHONUTF8", "1")
    sys.exit(main())
