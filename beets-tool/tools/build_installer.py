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

    # 依赖清单必须在包内（安装器从 data/ 读，见 install.requirements_file）
    data = SRC / "aiya_beets" / "data"
    data.mkdir(parents=True, exist_ok=True)
    lock = TOOL / "requirements.lock"
    if lock.is_file():
        shutil.copyfile(lock, data / "requirements.txt")

    # 清掉 __pycache__：它们会让包体积翻倍且与源码树时间戳不一致
    for pyc in SRC.rglob("__pycache__"):
        shutil.rmtree(pyc, ignore_errors=True)

    zipapp.create_archive(
        SRC,
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
