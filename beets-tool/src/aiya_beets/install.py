"""A 轨道安装器：用**本机已有的 Python** 建环境装 beets。

## 它解决的问题

播放器不代装 Python（docs/33 §10.1 拍板 3）。用户机器上已有 Python 时，
这条轨道最省事：建一个 venv、按 lock 装依赖、自检一次，落在
`{app_data}/beets-tool/venv`。

## 为什么是 venv 而不是装进系统 Python

不污染用户的全局环境，卸载 = 删目录。代价是 venv 不可重定位
（`pyvenv.cfg` 里的 `home` 是硬指针），所以它必须**在用户机器上现建**——
这正是本轨道与 B 轨道（自带解释器套件包，可重定位）的分工。

## 输出

行帧 JSON，与协议同风格（宿主逐行读、转成进度事件）。不用人类可读日志：
宿主按行解析 stdout，混进非 JSON 行就要靠容错兜。
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import venv
from pathlib import Path

from . import env

# Windows 上 pip / 解释器都是控制台程序，而调用它们的本进程是从播放器
# （GUI 子系统）经 CREATE_NO_WINDOW 拉起的 —— 自己**没有**控制台。
# 关键：父进程的 CREATE_NO_WINDOW **不会**传给子进程，所以每个子进程都会
# 被 Windows 分配一个新的黑窗（用户点一次「安装组件」能连看好几个框）。
# 每处 subprocess 都要单独补；POSIX 上没这个参数，用空 dict 绕过。
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def requirements_file(explicit: str | None = None) -> Path:
    """lock 文件位置。

    优先包内 `data/requirements.txt`（wheel 安装后唯一存在的副本）；
    开发时回落到仓库根的 `requirements.lock`——这样"在源码树里直接跑"
    和"装好的包里跑"行为一致，不用靠环境变量指路。
    """
    if explicit:
        return Path(explicit)
    bundled = Path(__file__).with_name("data") / "requirements.txt"
    if bundled.is_file():
        return bundled
    return Path(__file__).resolve().parents[2] / "requirements.lock"


def venv_python(target: Path) -> Path:
    """venv 里的解释器路径（Windows 是 Scripts，其他平台是 bin）。"""
    if os.name == "nt":
        return target / "Scripts" / "python.exe"
    return target / "bin" / "python"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aiya-beets install",
        description="在目标目录建 venv 并安装 beets（需要本机 Python）",
    )
    ap.add_argument("--target", required=True, help="venv 目标目录")
    ap.add_argument("--from-wheels", default=None, help="离线 wheelhouse（给了就不联网）")
    ap.add_argument("--requirements", default=None, help="依赖清单（默认包内 data/）")
    args = ap.parse_args(argv)

    target = Path(args.target)
    req = requirements_file(args.requirements)
    if not req.is_file():
        emit({"type": "error", "stage": "prepare", "message": f"缺少依赖清单：{req}"})
        return 2

    supported = env.runtime_supported()
    emit({
        "type": "progress", "stage": "prepare",
        "message": f"Python {env.runtime_version()}"
                   + ("" if supported else "（不在支持窗口内，仍尝试）"),
    })

    # 1) 建 venv（已存在则复用：重复安装就是升级/修复）
    py = venv_python(target)
    if not py.exists():
        emit({"type": "progress", "stage": "venv", "message": f"创建虚拟环境 {target}"})
        try:
            # with_pip=False + 自己跑 ensurepip：EnvBuilder 内部用
            # subprocess.check_output 调 ensurepip，没有地方传 creationflags，
            # 在无控制台的父进程下会单独闪一个黑窗。命令与它内部完全相同
            # （-Im ensurepip --upgrade --default-pip），只是句柄可控。
            venv.create(str(target), with_pip=False, clear=False)
            pip_setup = subprocess.run(
                [str(py), "-Im", "ensurepip", "--upgrade", "--default-pip"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                **_NO_WINDOW,
            )
            if pip_setup.returncode != 0:
                detail = (pip_setup.stderr or pip_setup.stdout).strip()[:300]
                raise RuntimeError(f"ensurepip 退出码 {pip_setup.returncode}：{detail}")
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
        **_NO_WINDOW,
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
        **_NO_WINDOW,
    )
    if check.returncode != 0:
        emit({"type": "error", "stage": "verify", "message": check.stderr.strip()[:500]})
        return 6

    emit({"type": "done", "stage": "done", "python": str(py),
          "beetsVersion": check.stdout.strip()})
    return 0
