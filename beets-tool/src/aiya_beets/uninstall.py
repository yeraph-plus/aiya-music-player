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

## 两种目标（`--all`）

    uninstall --target {data_root}/beets-tool/venv     # A 轨道：只删 venv
    uninstall --target {data_root}/beets-tool --all    # 整层：venv + bundle + bin

`--all` 是宿主用的形态：卸载要撤回"安装往这一层放的**全部**东西"，而
bin/ 里的外置工具（docs/53）和 bundle/ 里的套件包都不在 venv 内。
不给 `--all` 就只认 venv——手工命令行误传 `{data_root}` 时不会连带删掉数据。

## 安全闸

这个脚本会 `rmtree`，参数来自外部（宿主/命令行）。所以：
1. 必须是**已存在**的目录，且 resolves 后仍是它自己（不被 junction 骗）；
2. 必须看起来是安装产物（venv 有 `pyvenv.cfg`；`--all` 认 `beets-tool` 目录名
   或 `venv/` `bundle/` `bin/` 任一存在），否则拒绝——防止把 `D:\\Music` 传进来；
3. 拒绝盘根 / 用户主目录 / Windows 与 Program Files；
4. **拒绝删除自己所在的目录树**：Windows 上运行中的 exe 删不掉（实测
   `DeleteFileW` 返回 0、错误码 5 access denied，同目录的普通文件返回 1 可删）。
   从 venv 里跑 `uninstall --target <venv>` 必然删一半就失败，所以真删时直接
   拒绝并说明"由外部执行"。`--dry-run` 不受这条限制——**宿主正是靠它拿计划**。
5. 先统计再删，把"删了什么、多少字节"报给宿主（UI 要显示，用户要能判断）。

## 为什么宿主只借用 `--dry-run`

`aiya-beets uninstall` 的执行体就在被删目录里（第 4 条），所以"交接给工具侧删"
在 Windows 上不成立。实际分工是：**工具侧出计划并过闸，宿主执行删除**。
这样四道闸照旧生效，而删除动作由不在目录树内的进程做。
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


def _looks_like_install(target: Path, everything: bool) -> str | None:
    """返回拒绝理由；None = 看起来是安装产物。

    `--all` 放宽到"目录名是 beets-tool，或含 venv/bundle/bin 任一"：整层卸载的
    目标本来就是 `beets-tool` 本身，它没有 `pyvenv.cfg`（那是 venv 才有的）。
    放宽的两条都是**安装器自己造出来的形状**，不是任何目录都可能满足的。
    """
    if everything:
        if target.name == "beets-tool":
            return None
        if any((target / name).is_dir() for name in ("venv", "bundle", "bin")):
            return None
        return f"不像 beets-tool 安装目录（既不是 beets-tool，也没有 venv/bundle/bin）：{target}"
    # 只认 venv 形态：`pyvenv.cfg` 是 venv 的定义文件。没有它就不是安装器
    # 造出来的东西，删掉等于替用户决定他的目录该不该留。
    if (target / "pyvenv.cfg").is_file() or venv_python(target).is_file():
        return None
    return f"不像是安装器建的环境（没有 pyvenv.cfg）：{target}"


def _contains_self(target: Path) -> bool:
    """目标目录树里有没有**当前解释器**。

    Windows 上运行中的 exe 持有映像节锁，`rmtree` 到它必然失败（实测
    `DeleteFileW` 错误码 5）。从 venv 里卸载 venv 就会撞上——那不是"用户没退出
    应用"这类可恢复的错，是结构上不可能成功，所以要提前拒，别删一半。
    """
    try:
        Path(sys.executable).resolve().relative_to(target)
        return True
    except (ValueError, OSError):
        return False


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aiya-beets uninstall",
        description="删除安装产物（依赖 / beets / 插件 / 外置工具）；数据目录不动",
    )
    ap.add_argument("--target", required=True, help="install 时的 --target，或 beets-tool 整层（配 --all）")
    ap.add_argument(
        "--all",
        action="store_true",
        help="目标是 beets-tool 整层（含 venv / bundle / bin），不只是 venv",
    )
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

    why = _looks_like_install(target, args.all)
    if why:
        emit({"type": "error", "stage": "prepare", "message": f"拒绝卸载：{why}"})
        return 2

    files, size = _measure(target)
    emit({
        "type": "progress", "stage": "prepare",
        "message": f"将删除 {target}（{files} 个文件，{size / 2**20:.1f} MB）",
    })
    if args.dry_run:
        # 计划里带上 all：宿主据此知道这次核算覆盖的是整层还是单个 venv
        emit({"type": "done", "stage": "done", "removed": False, "target": str(target),
              "files": files, "bytes": size, "dryRun": True, "all": args.all})
        return 0

    if _contains_self(target):
        emit({
            "type": "error", "stage": "prepare",
            "message": (
                f"拒绝删除：当前解释器就在目标目录内（{sys.executable}）。"
                "Windows 上运行中的 exe 删不掉，请由宿主或另一个目录里的脚本执行"
            ),
        })
        return 2

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
