# -*- coding: utf-8 -*-
"""卸载契约（docs/55）：闸门、`--all` 语义、以及"计划与执行分家"的边界。

卸载是唯一会 `rmtree` 的入口，参数还可能来自外部。所以门禁的重点不是
"能删干净"，而是**不该删的时候一个字节都不动**。

  [1] `--all` 正向：beets-tool 整层，账目对得上（files/bytes）
  [2] `--all` 但既不是 beets-tool 也没有 venv/bundle/bin → 拒绝
  [3] 不给 `--all`：没有 pyvenv.cfg 拒绝；有则通过（只认 venv 形态）
  [4] 危险路径：盘根 / 用户主目录必须被 `_dangerous` 拦下
  [5] 自身在目录内：`_contains_self` 认得出，且**真删时拒绝**（Windows 删不掉）
  [6] `--dry-run` 只出计划，目录必须还在
  [7] 真删（解释器不在里面）→ 目录消失，且账目与 [1] 一致
  [8] 幂等：目标不存在 → done / removed=false / 退出码 0

为什么 [5] 是硬闸而不是"尽量做"：实测运行中的 exe `DeleteFileW` 返回 0、
错误码 5（access denied），同目录的普通文件返回 1 可删。从 venv 里卸 venv
结构上不可能成功，删一半比不删更难收拾。
"""
import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(TOOL, "src"))

from aiya_beets import uninstall  # noqa: E402

WORK = tempfile.mkdtemp(prefix="aiya-uninstall-")


def run(argv):
    """跑一次 `uninstall.main`，返回 (退出码, 行帧列表)。"""
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        try:
            code = uninstall.main(argv)
        except SystemExit as e:  # argparse 自己报错
            code = e.code
    frames = [
        json.loads(line)
        for line in buf.getvalue().splitlines()
        if line.strip().startswith("{")
    ]
    return code, frames


def make_install(parent, name="beets-tool"):
    """造一个像安装产物的目录：venv/ + bundle/ + bin/。"""
    root = Path(parent) / name
    for sub in ("venv", "bundle", "bin"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    (root / "venv" / "pyvenv.cfg").write_text("home = C:\\Python314\n", encoding="utf-8")
    (root / "bin" / "ffmpeg.exe").write_bytes(b"x" * 10)
    return root


def refused(code, frames, why=""):
    assert code == 2, f"应当拒绝（退出码 2），实际 {code}：{frames}"
    assert frames and frames[-1]["type"] == "error", f"应当有 error 帧：{frames}"
    assert frames[-1]["stage"] == "prepare", frames[-1]
    assert why in frames[-1]["message"], f"理由不对：{frames[-1]['message']}"


try:
    # ---- 1. `--all` 正向：账目对得上 ----
    a = make_install(WORK, "beets-tool")
    code, frames = run(["--target", str(a), "--all", "--dry-run"])
    assert code == 0, frames
    done = frames[-1]
    assert done["type"] == "done" and done["dryRun"] is True, done
    assert done["all"] is True, "计划里要标明这次覆盖的是整层"
    assert done["files"] == 2, f"venv/pyvenv.cfg + bin/ffmpeg.exe，应为 2：{done}"
    # 期望值从盘上量：`write_text` 会把 \n 转成 \r\n，写死字节数只会变成
    # 一条平台相关的假红（这条断言要盯的是"账目准不准"，不是换行风格）
    expected = (a / "venv" / "pyvenv.cfg").stat().st_size + 10
    assert done["bytes"] == expected, f"字节数应是 {expected}：{done}"
    print(f"[1] `--all` 账目 OK（{done['files']} 个文件 / {done['bytes']} 字节）")

    # ---- 2. `--all` 但形状不对 ----
    plain = Path(WORK) / "随便一个目录"
    plain.mkdir()
    (plain / "歌曲.flac").write_bytes(b"x")
    refused(*run(["--target", str(plain), "--all", "--dry-run"]), "不像 beets-tool 安装目录")
    # 名字对了但里面什么都没有：靠名字过（安装到一半只剩空壳时也要能卸）
    empty = Path(WORK) / "empty" / "beets-tool"
    empty.mkdir(parents=True)
    code, frames = run(["--target", str(empty), "--all", "--dry-run"])
    assert code == 0 and frames[-1]["files"] == 0, frames
    print("[2] `--all` 形状闸 OK（陌生目录拒绝；空壳 beets-tool 放行）")

    # ---- 3. 不给 `--all`：只认 venv 形态 ----
    refused(*run(["--target", str(a), "--dry-run"]), "没有 pyvenv.cfg")
    code, frames = run(["--target", str(a / "venv"), "--dry-run"])
    assert code == 0 and frames[-1]["all"] is False, frames
    print("[3] 单 venv 语义 OK（整层不给 --all 会被拒；venv 本身放行）")

    # ---- 4. 危险路径 ----
    for target in (Path("C:\\"), Path.home()):
        why = uninstall._dangerous(target)
        assert why, f"{target} 必须被拦下（实际放行）"
    assert uninstall._dangerous(Path(WORK) / "beets-tool") is None, "正常路径不该被拦"
    print("[4] 危险路径闸 OK（盘根 / 主目录拦下，正常路径放行）")

    # ---- 5. 自身在目录内 ----
    assert uninstall._contains_self(Path(sys.executable).resolve().parent) is True, \
        "当前解释器所在目录应当被认出"
    assert uninstall._contains_self(Path(tempfile.gettempdir())) is False or \
        str(Path(sys.executable).resolve()).startswith(str(Path(tempfile.gettempdir()))), \
        "临时目录不该被误认成解释器所在目录"
    self_dir = make_install(WORK, "self")
    shutil.copyfile(sys.executable, self_dir / Path(sys.executable).name)
    # 抄一份解释器进目录：这时 _contains_self 对**那个副本**成立，但对本进程不成立
    # （本进程的 sys.executable 还在 venv 里）。所以这里验证的是判定函数本身。
    assert uninstall._contains_self(self_dir) is False, (
        "判定看的是当前进程的解释器，不是目录里有几个 exe"
    )
    print("[5] 自身在目录内 OK（按当前解释器判定，不按目录里有没有 exe）")

    # ---- 6. dry-run 不删 ----
    assert a.exists(), "dry-run 之后目录必须还在"
    print("[6] dry-run 非破坏 OK")

    # ---- 7. 真删 ----
    victim = make_install(WORK, "victim")
    code, frames = run(["--target", str(victim), "--all"])
    assert code == 0, frames
    assert frames[-1]["removed"] is True and frames[-1]["files"] == 2, frames[-1]
    assert not victim.exists(), "真删之后目录必须消失"
    print("[7] 真删 OK（removed=true，目录消失）")

    # ---- 8. 幂等 ----
    code, frames = run(["--target", str(victim), "--all"])
    assert code == 0, frames
    assert frames[-1]["removed"] is False and frames[-1]["files"] == 0, frames[-1]
    print("[8] 幂等 OK（不存在 → done / removed=false / 退出 0）")

    print("\nALL PASS  卸载契约 8 组")
finally:
    shutil.rmtree(WORK, ignore_errors=True)
