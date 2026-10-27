#!/usr/bin/env python
"""AIYA Music — beets 运行时装到应用数据目录（docs/29 批 C）。

## 为什么不是"随包带 venv"

docs/29 §5 原本的路线 B 是"CI 预构建 venv，整个目录打进安装包"。**实测不成立**：
Windows 的 venv 不自包含，`pyvenv.cfg` 里 `home = C:\\Python314` 指向建库时的
系统 Python，标准库和 python314.dll 都在那儿。把 venv 拷到另一台没有同版本
Python 的机器上，`python.exe` 直接报
`did not find executable at '...\\python.exe'`。

真正能随包分发的形态是 **embeddable Python + 独立 site-packages 目录**
（`python-3.14.7-embed-amd64.zip` 12MB，自带解释器和标准库，靠 `python314._pth`
把额外目录加进 sys.path）。那条路由 `tools/pack-beets-runtime.mjs` 在构建期做，
产物进安装包，开箱即用。

## 那这个脚本做什么

覆盖"随包运行时缺失/损坏，但用户机器有 Python"这条路：
用用户的系统 Python 建一个 venv，把 requirements 装进去，落在
`{app_data}/beets-venv`。宿主探测顺序里有这个位置，装完即可用。

## 用法

    python beets_install.py --target <dir>                      # 在线（PyPI）
    python beets_install.py --target <dir> --from-wheels <dir>  # 离线 wheelhouse

输出是**行帧 JSON**（与 beets_sidecar.py 同风格）：宿主逐行读、转成进度事件。
不用 print 打人类可读日志——宿主按行解析 stdout，混进非 JSON 行就要靠容错。
"""

import argparse
import json
import os
import subprocess
import sys
import venv
from pathlib import Path


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _req_file(explicit: str | None) -> Path:
    """requirements 路径。

    生产路径：宿主把 `beets_requirements.txt` 落在 sidecar 脚本同目录（见
    beets.rs::ensure_script），所以默认读同目录。
    `--requirements` 是给"手动/CI 直接跑这个脚本"用的：那种场景没有宿主落盘，
    直接指向仓库里的 `tools/beets-requirements.lock`（唯一真源）。
    """
    if explicit:
        return Path(explicit)
    return Path(__file__).with_name("beets_requirements.txt")


def _venv_python(target: Path) -> Path:
    """venv 里的解释器路径。Windows 是 Scripts，其他平台是 bin（本应用只发 Windows，
    但脚本保持可移植，方便在 Linux CI 上跑冒烟）。"""
    if os.name == "nt":
        return target / "Scripts" / "python.exe"
    return target / "bin" / "python"


def main() -> int:
    ap = argparse.ArgumentParser(description="安装 beets 到应用数据目录（venv）")
    ap.add_argument("--target", required=True, help="venv 目标目录（{app_data}/beets-venv）")
    ap.add_argument("--from-wheels", default=None, help="离线 wheelhouse 目录（给了就不联网）")
    ap.add_argument("--requirements", default=None, help="依赖清单路径（默认脚本同目录）")
    args = ap.parse_args()

    target = Path(args.target)
    req = _req_file(args.requirements)
    if not req.is_file():
        emit({"type": "error", "stage": "prepare", "message": f"缺少 requirements：{req}"})
        return 2

    # 1) 建 venv（已存在则复用：重复安装就是升级/修复）
    py = _venv_python(target)
    if not py.exists():
        emit({"type": "progress", "stage": "venv", "message": f"创建虚拟环境 {target}"})
        try:
            # with_pip=True 走 ensurepip（系统 Python 自带），不联网
            venv.create(str(target), with_pip=True, clear=False)
        except Exception as e:  # noqa: BLE001 —— 任何失败都要报给 UI，不能静默
            emit({"type": "error", "stage": "venv", "message": f"{type(e).__name__}: {e}"})
            return 3
        if not py.exists():
            emit({"type": "error", "stage": "venv", "message": f"venv 建完但找不到 {py}"})
            return 3

    # 2) 装依赖
    cmd = [str(py), "-m", "pip", "install", "--disable-pip-version-check", "-r", str(req)]
    if args.from_wheels:
        wheel_dir = Path(args.from_wheels)
        if not wheel_dir.is_dir():
            emit({"type": "error", "stage": "install", "message": f"wheelhouse 不存在：{wheel_dir}"})
            return 4
        # --no-index：给了 wheelhouse 就**绝不**回落到 PyPI，
        # 否则离线场景会卡在网络上而不是干净地报"缺 wheel"
        cmd += ["--no-index", "--find-links", str(wheel_dir)]
    emit({"type": "progress", "stage": "install", "message": "pip install 开始"})

    # 逐行透传 pip 输出：安装要几十秒到几分钟，没有进度用户会以为卡死
    with subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
    ) as proc:
        assert proc.stdout is not None
        for line in proc.stdout:
            line = line.rstrip()
            if line:
                emit({"type": "progress", "stage": "pip", "message": line})
        code = proc.wait()

    if code != 0:
        emit({"type": "error", "stage": "pip", "message": f"pip 退出码 {code}"})
        return 5

    # 3) 自检：真 import 一次。只看 pip 退出码不够——装到一半的依赖也能返回 0
    check = subprocess.run(
        [str(py), "-c", "import beets; print(beets.__version__)"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
    )
    if check.returncode != 0:
        emit({"type": "error", "stage": "verify", "message": check.stderr.strip()[:500]})
        return 6

    emit({"type": "done", "stage": "done", "python": str(py), "beetsVersion": check.stdout.strip()})
    return 0


if __name__ == "__main__":
    sys.exit(main())
