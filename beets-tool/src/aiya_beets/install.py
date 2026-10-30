"""A 轨道安装器：用**本机已有的 Python** 建环境，按插件清单装 beets。

## 它解决的问题

播放器不代装 Python（docs/33 §10.1 拍板 3）。用户机器上已有 Python 时，
这条轨道最省事：建一个 venv、按 lock 装依赖、把用户勾选的插件依赖一并装上、
自检一次，落在 `{app_data}/beets-tool/venv`。

## 为什么是 venv 而不是装进系统 Python

不污染用户的全局环境，卸载 = 删目录。代价是 venv 不可重定位
（`pyvenv.cfg` 里的 `home` 是硬指针），所以它必须**在用户机器上现建**——
这正是本轨道与 B 轨道（自带解释器套件包，可重定位）的分工。

## 入参模式（docs/51 §2）

    install --target DIR [--plugins a,b] [--requirements F] [--from-wheels D] [--dry-run]

`--plugins` 给的是**插件名**，不是 pip 包名：装什么由 `data/plugins.json`
决定（`plugins.packages_for`），脚本不维护第二份依赖表。未知名字直接报错
退出——静默跳过会让用户"勾了 A 装了 B"，比报错难查十倍。

## 为什么读包内资源要用 importlib.resources

`.pyz` 是 zip，`Path(__file__).parent/'data'` 在 zip 里 `is_file()` 恒 False。
旧版就是这么写的，于是安装器回落到 `dist/requirements.lock`，报
「缺少依赖清单」——**A 轨道在真机上从来没装成过**（docs/51 §0 实测）。
pip 只认真实文件路径，所以包内资源先落到临时目录再交给它。

## 输出

行帧 JSON，与协议同风格（宿主逐行读、转成进度事件）。不用人类可读日志：
宿主按行解析 stdout，混进非 JSON 行就要靠容错兜。
"""

from __future__ import annotations

import argparse
import atexit
import json
import os
import shutil
import subprocess
import sys
import tempfile
import venv
from importlib.resources import files
from pathlib import Path

from . import bins, env, plugins

# Windows 上 pip / 解释器都是控制台程序，而调用它们的本进程是从播放器
# （GUI 子系统）经 CREATE_NO_WINDOW 拉起的 —— 自己**没有**控制台。
# 关键：父进程的 CREATE_NO_WINDOW **不会**传给子进程，所以每个子进程都会
# 被 Windows 分配一个新的黑窗（用户点一次「安装组件」能连看好几个框）。
# 每处 subprocess 都要单独补；POSIX 上没这个参数，用空 dict 绕过。
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}

_TMP_DIRS: list[Path] = []


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def venv_python(target: Path) -> Path:
    """venv 里的解释器路径（Windows 是 Scripts，其他平台是 bin）。"""
    if os.name == "nt":
        return target / "Scripts" / "python.exe"
    return target / "bin" / "python"


def _materialize(name: str) -> Path | None:
    """把包内 `data/<name>` 落成真实文件，返回路径（找不到返回 None）。

    为什么必须有这一步：pip 的 `-r` 只吃真实路径，而 `.pyz` 里的文件在 zip 中，
    没有可传给子进程的路径。落到临时目录后一次安装用完即删（atexit）。
    """
    try:
        res = files("aiya_beets").joinpath(f"data/{name}")
        text = res.read_text(encoding="utf-8")
    except (FileNotFoundError, ModuleNotFoundError, OSError, UnicodeDecodeError):
        return None
    tmp = Path(tempfile.mkdtemp(prefix="aiya-install-"))
    _TMP_DIRS.append(tmp)
    out = tmp / name
    out.write_text(text, encoding="utf-8", newline="\n")
    return out


def requirements_file(explicit: str | None = None) -> Path | None:
    """依赖清单位置。

    优先级：显式参数 > 包内 `data/requirements.txt` > 源码树的 `requirements.lock`。
    包内那份是 `.pyz` 唯一能看到的副本（build_installer 把 lock 拷成
    `data/requirements.txt`）；源码树里没有它时走第三条，方便直接跑脚本调试。
    """
    if explicit:
        return Path(explicit)
    bundled = _materialize("requirements.txt")
    if bundled is not None:
        return bundled
    dev = Path(__file__).resolve().parents[2] / "requirements.lock"
    return dev if dev.is_file() else None


def parse_plugins(raw: str | None) -> list[str]:
    """`--plugins a,b` → 名字清单（去重保序，空串忽略）。

    `all` 是保留字：装目录里所有"不需要外部二进制"的插件，
    对应 UI 的「全选」。
    """
    items = [x.strip() for x in (raw or "").split(",")]
    names = [x for x in items if x]
    if names == ["all"]:
        return [i["name"] for i in plugins.entries() if not i.get("exe")]
    seen: list[str] = []
    for n in names:
        if n not in seen:
            seen.append(n)
    return seen


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aiya-beets install",
        description="在目标目录建 venv 并安装 beets（需要本机 Python）",
    )
    ap.add_argument("--target", required=True, help="venv 目标目录")
    ap.add_argument("--plugins", default=None, help="要启用的插件名，逗号分隔（all = 全部免外部依赖的）")
    ap.add_argument("--from-wheels", default=None, help="离线 wheelhouse（给了就不联网）")
    ap.add_argument("--requirements", default=None, help="依赖清单（默认包内 data/）")
    ap.add_argument("--dry-run", action="store_true", help="只解析并打印安装计划，不落盘")
    args = ap.parse_args(argv)

    target = Path(args.target)
    req = requirements_file(args.requirements)
    if req is None or not req.is_file():
        emit({"type": "error", "stage": "prepare", "message": f"缺少依赖清单：{req}"})
        return 2

    # 插件解析放在最前面：名字错了要立刻报，不能等 pip 跑完两分钟才说
    names = parse_plugins(args.plugins)
    try:
        extra = plugins.packages_for(names)
        need_mods = plugins.imports_for(names)
        # 勾了 replaygain / convert / chroma / xtractor，就得把它们要的外部
        # 二进制一起带上（docs/53）：缺 ffmpeg 不是"少个功能"，是 beets 整个
        # CLI 起不来。谁需要谁带，不给用户第二次点击。
        need_bins = plugins.binaries_for(names)
    except KeyError as e:
        known = ", ".join(s.name for s in plugins.MARKET_CATALOG)
        emit({
            "type": "error", "stage": "prepare",
            "message": f"不认识的插件 {e}（可用：{known}）",
        })
        return 2

    plan = {
        "target": str(target),
        "requirements": str(req),
        "plugins": names,
        "packages": extra,
        "checkModules": need_mods,
        "binaries": need_bins,
    }
    if args.dry_run:
        emit({"type": "dryRun", "stage": "prepare", **plan})
        emit({"type": "done", "stage": "done", "dryRun": True,
              "installedPlugins": names, "packages": extra, "binaries": need_bins})
        return 0

    supported = env.runtime_supported()
    emit({
        "type": "progress", "stage": "prepare",
        "message": f"Python {env.runtime_version()}"
                   + ("" if supported else "（不在支持窗口内，仍尝试）"),
    })
    if names:
        emit({
            "type": "progress", "stage": "prepare",
            "message": f"插件 {', '.join(names)}"
                       + (f" → 额外包 {', '.join(extra)}" if extra else "（无需额外包）"),
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

    # 2) 装依赖：lock + 插件包**一次**装完。
    # 分两次装会让 pip 做两次解析，第二次可能把第一次钉好的版本降级。
    cmd = [str(py), "-m", "pip", "install", "--disable-pip-version-check", "-r", str(req)]
    cmd += extra
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
    probe = "import beets; print(beets.__version__)"
    check = subprocess.run(
        [str(py), "-c", probe],
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

    # 4) 插件自检：勾了 lyrics 却 import 不到 bs4，等于白装。
    # `beetsplug.<name>` 存在 ≠ 依赖齐（beets 对插件导入异常只写日志）。
    missing: list[str] = []
    for mod in need_mods:
        got = subprocess.run(
            [str(py), "-c", f"import {mod}"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
            **_NO_WINDOW,
        )
        if got.returncode != 0:
            missing.append(mod)
    if missing:
        emit({
            "type": "error", "stage": "verify",
            "message": f"装完仍缺模块：{', '.join(missing)}（插件要的依赖没装上）",
        })
        return 6

    # 5) 外部二进制：勾了需要它们的插件才下载。beets 本体此时已经可用，
    # 所以这一步失败不回滚，只报"工具没下下来"（可重试 bins）。
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
            emit({
                "type": "error", "stage": "bin",
                "message": f"beets 已装好，但外置工具没下下来：{e}",
            })
            return 7

    emit({
        "type": "done", "stage": "done",
        "python": str(py),
        "beetsVersion": check.stdout.strip(),
        "installedPlugins": names,
        "packages": extra,
        "checkModules": need_mods,
        "binaries": bin_rows,
    })
    return 0


def _cleanup() -> None:
    for d in _TMP_DIRS:
        shutil.rmtree(d, ignore_errors=True)


atexit.register(_cleanup)
