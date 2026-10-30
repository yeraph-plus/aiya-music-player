"""A 轨道的卸载：`install` 的反向操作，**只删它装进去的东西**。

## 边界（2026-10-30 拍板）

安装脚本装了四类东西到 `--target`：venv 解释器、beets 本体、pip 依赖、
用户勾选的插件。这四类都在同一个目录里，所以"反向操作"就是删这个目录。

**数据不归它管**：`beets/`（整理家的 library.db / config / ops.jsonl）与
`beets-index/`（分析库 / enrich.db）虽然也在应用数据目录内，但它们是用户
资产、不是安装产物，且应用自己有清理入口。卸载组件顺手删掉 1850 首的分析
结果，是那种"用户没让你做"的破坏。

## 为什么删目录而不是 `pip uninstall`

venv 是私有的：删目录既彻底又不依赖 pip 的状态。逐包 uninstall 会漏掉
`__pycache__`、空的 `bin/`、以及 pip 自己不认识的残留（比如 ——target 装进去的
`aiya_beets` 源码目录）。

## 安全闸

这个脚本会 `rmtree`，参数来自外部（宿主/命令行）。所以：
1. 必须是**已存在**的目录，且 resolves 后仍是它自己（不被 junction 骗）；
2. 必须看起来是 venv（有 `pyvenv.cfg`），否则拒绝——防止把 `D:\\Music` 传进来；
3. 拒绝盘根 / 用户主目录 / Windows 与 Program Files；
4. 先统计再删，把"删了什么、多少字节"报给宿主（UI 要显示，用户要能判断）。
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

from .install import venv_python


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _dangerous(target: Path) -> str | None:
    """返回拒绝理由；None = 可以删。"""
    parts = target.parts
    # 盘根（Windows: ("C:\\",) / POSIX: ("/",)）
    if len(parts) <= 1:
        return "目标是盘根"
    home = Path.home().resolve()
    try:
        if target.resolve() == home:
            return "目标是用户主目录"
    except OSError:
        pass
    for env_key in ("SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"):
        val = os.environ.get(env_key)
        if not val:
            continue
        try:
            sysdir = Path(val).resolve()
        except OSError:
            continue
        try:
            if target.resolve() == sysdir or sysdir in target.resolve().parents:
                return f"目标在系统目录 {sysdir} 内"
        except OSError:
            pass
    return None


def _measure(target: Path) -> tuple[int, int]:
    """(文件数, 字节数)。先把账算清再删——删完就没法报了。"""
    files = 0
    size = 0
    for root, _dirs, names in os.walk(target):
        for name in names:
            p = Path(root) / name
            try:
                if p.is_symlink():
                    continue  # 不跟随，也不算它的体积
                size += p.stat().st_size
                files += 1
            except OSError:
                continue
    return files, size


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aiya-beets uninstall",
        description="删除安装器建的环境（依赖 / beets / 插件）；数据目录不动",
    )
    ap.add_argument("--target", required=True, help="install 时的 --target（venv 目录）")
    ap.add_argument("--dry-run", action="store_true", help="只报告会删什么，不真删")
    args = ap.parse_args(argv)

    raw = Path(args.target)
    if not raw.exists():
        # 幂等：已经没有了就是成功，不是错误（重装前先卸载的脚本会撞到这点）
        emit({"type": "done", "stage": "done", "removed": False,
              "target": str(raw), "files": 0, "bytes": 0,
              "message": "目录不存在，无需卸载"})
        return 0
    if raw.is_symlink():
        emit({"type": "error", "stage": "prepare",
              "message": f"目标是符号链接，拒绝跟随：{raw}"})
        return 2

    target = raw.resolve()
    why = _dangerous(target)
    if why:
        emit({"type": "error", "stage": "prepare", "message": f"拒绝卸载：{why}（{target}）"})
        return 2

    # 只认 venv 形态：`pyvenv.cfg` 是 venv 的定义文件。没有它就不是安装器
    # 造出来的东西，删掉等于替用户决定他的目录该不该留。
    looks_like_venv = (target / "pyvenv.cfg").is_file() or venv_python(target).is_file()
    if not looks_like_venv:
        emit({
            "type": "error", "stage": "prepare",
            "message": f"不像是安装器建的环境（没有 pyvenv.cfg）：{target}",
        })
        return 2

    files, size = _measure(target)
    emit({
        "type": "progress", "stage": "prepare",
        "message": f"将删除 {target}（{files} 个文件，{size / 2**20:.1f} MB）",
    })
    if args.dry_run:
        emit({"type": "done", "stage": "done", "removed": False, "target": str(target),
              "files": files, "bytes": size, "dryRun": True})
        return 0

    try:
        shutil.rmtree(target)
    except OSError as e:
        # 最常见的原因：sidecar 还在跑、或用户的杀软正扫着目录
        emit({
            "type": "error", "stage": "remove",
            "message": f"删除失败（{type(e).__name__}: {e}）。请先退出应用再试",
        })
        return 3
    if target.exists():
        emit({"type": "error", "stage": "remove", "message": f"删除后目录仍存在：{target}"})
        return 3

    emit({"type": "done", "stage": "done", "removed": True, "target": str(target),
          "files": files, "bytes": size,
          "message": "环境已删除；整理库与分析库未改动（由应用自己管理）"})
    return 0
