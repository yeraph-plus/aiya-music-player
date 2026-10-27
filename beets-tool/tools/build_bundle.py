#!/usr/bin/env python
"""构建 B 轨道发布物：`aiya-beets-bundle-win-x64.zip`（免安装套件包）。

## 它解决什么

用户机器上**没有 Python**、也不该被要求去装一个。套件包把运行时整个搬过来：
自带解释器 + 标准库 + DLL + 依赖 + 我们的包。

## 为什么可重定位（这是 B 轨道成立的前提）

Python 官方的 embeddable 发行版用 `python3xx._pth` 列出搜索路径（全部相对
解释器所在目录），`sys.prefix` 由 exe 位置推出 ⇒ 整个目录复制到任意路径都能跑。
这与 venv 相反：venv 的 `pyvenv.cfg` 里 `home` 是**绝对路径硬指针**，换个位置
就废（docs/29 批 C 实测，docs/33 §6.2）。

## 为什么 `_pth` 决定包放哪

`_pth` 存在时 Python **忽略 PYTHONPATH**（实测：只设 PYTHONPATH 时
`python.exe -m aiya_beets` 报 No module named aiya_beets）。所以我们的包必须
落在 `_pth` 列出的目录里 —— 当前是 `Lib\\site-packages`。
这也是本脚本不做"临时拼 sys.path"的原因：拼了在 embeddable 上根本不生效。
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent          # beets-tool/tools
TOOL = HERE.parent                              # beets-tool
REPO = TOOL.parent
SRC = TOOL / "src"
VERSION = "0.1.0"


def _ignore(_dir: str, names: list[str]) -> set[str]:
    return {n for n in names if n == "__pycache__" or n.endswith(".pyc")}


def main() -> int:
    ap = argparse.ArgumentParser(description="打包 aiya-beets 免安装套件包")
    ap.add_argument("--runtime", default=str(REPO / "tools" / "beets-runtime"),
                    help="自带解释器目录（含 python.exe 与 Lib/site-packages）")
    ap.add_argument("--out", default=str(TOOL / "dist" / "aiya-beets-bundle-win-x64.zip"))
    ap.add_argument("--work", default=str(TOOL / "dist" / "bundle"))
    ap.add_argument("--no-zip", action="store_true", help="只展开目录，不打包（调试用）")
    args = ap.parse_args()

    runtime = Path(args.runtime)
    if not (runtime / "python.exe").is_file():
        print(f"运行时目录不完整（缺 python.exe）：{runtime}", file=sys.stderr)
        return 2

    work = Path(args.work)
    if work.exists():
        shutil.rmtree(work)
    work.parent.mkdir(parents=True, exist_ok=True)

    print(f"展开运行时 → {work}")
    shutil.copytree(runtime, work, ignore=_ignore)

    # 包必须进 _pth 列出的目录（见模块注释）
    site = work / "Lib" / "site-packages"
    site.mkdir(parents=True, exist_ok=True)
    print("安装 aiya_beets 包 → Lib/site-packages")
    shutil.copytree(SRC / "aiya_beets", site / "aiya_beets", dirs_exist_ok=True, ignore=_ignore)

    manifest = {
        "name": "aiya-beets",
        "version": VERSION,
        "protocolVersion": 3,
        "entry": "python.exe",
        "args": ["-m", "aiya_beets"],
        "note": "entry 相对解压根；播放器据此记下启动入口",
    }
    (work / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n"
    )

    # 自检：用**包里的**解释器跑，证明可执行（这一步在老机器上会抓出缺 DLL）
    probe = subprocess.run(
        [str(work / "python.exe"), "-m", "aiya_beets", "version"],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        # 继承当前环境（缺 SYSTEMROOT 等基础变量时解释器起不来，看起来像"包坏了"）
        env={**__import__("os").environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
    )
    if probe.returncode != 0 or not probe.stdout.strip():
        print(f"套件包自检失败：{probe.stdout} {probe.stderr[:400]}", file=sys.stderr)
        return 1
    print(f"自检通过：{probe.stdout.strip()}")

    if args.no_zip:
        print(f"OK（未打包） {work}")
        return 0

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    print(f"压缩 → {out}（{sum(f.stat().st_size for f in work.rglob('*') if f.is_file()) / 2**20:.0f} MB 待压缩）")
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as zf:
        for path in sorted(work.rglob("*")):
            if path.is_file():
                zf.write(path, path.relative_to(work).as_posix())
    print(f"OK  {out}  {out.stat().st_size / 2**20:.1f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
