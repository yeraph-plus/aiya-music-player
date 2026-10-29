# -*- coding: utf-8 -*-
"""库文件锁（v2）冒烟：跨进程互斥、holder 上报、进程死亡自动释放。

真进程验证（锁的语义只有两个进程才说得清）：
  1. 持有者进程活着 → 本进程 acquire 必须被拒（E_LIBRARY_BUSY，holder 可读）；
  2. 持有者被杀（模拟崩溃）→ 锁由 OS 释放，本进程立刻能拿到——短进程模型
     "锁随进程生死"的红利，不需要 TTL/心跳；
  3. probe 的三态：free（无锁文件）/ held（别人拿着）/ held（自己拿着，
     acquire 之后的 probe 必须报自己）。

隔离运行：独立临时 AIYA_BEETS_HOME，不碰真实数据目录。
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
TOOL_SRC = os.path.join(TOOL, "src")
PYTHON = os.environ.get("AIYA_BEETS_PYTHON") or os.path.join(
    TOOL, ".venv", "Scripts", "python.exe")
sys.path.insert(0, TOOL_SRC)

WORK = tempfile.mkdtemp(prefix="aiya-lock-")
HOME = os.path.join(WORK, "home")
os.makedirs(HOME)
os.environ["AIYA_BEETS_HOME"] = HOME  # 主进程与子进程必须指向同一个锁目录

ENV = dict(os.environ, AIYA_BEETS_HOME=HOME, PYTHONIOENCODING="utf-8", PYTHONUTF8="1")
ENV.pop("BEETSDIR", None)  # 锁路径只认 AIYA_BEETS_HOME，别让外部 BEETSDIR 干扰

# 持有者进程：acquire 后睡 30s（等主进程做完断言，由主进程树杀）。
# 注意 uv 建的 venv，python.exe 是 trampoline——真身是它派生的子进程，
# 所以持有者自己上报 pid（holder 文件里的就是它），主进程按它断言；
# "杀"也要用 taskkill /T 树杀，只 kill trampoline 会留下真身继续持锁。
HOLDER = (
    "import sys, time;"
    "sys.path.insert(0, r'" + TOOL_SRC + "');"
    "from aiya_beets import lockfile;"
    "lockfile.acquire('apply_import');"
    "import os; print('HELD', os.getpid(), flush=True);"
    "time.sleep(30)"
)

from aiya_beets import lockfile  # noqa: E402
from aiya_beets.protocol import E_LIBRARY_BUSY, ProtocolError  # noqa: E402


def kill_tree(proc) -> None:
    """树杀持有者（Windows 的 venv trampoline 有子进程；POSIX 直接 kill）。"""
    if os.name == "nt":
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                       capture_output=True)
    else:
        proc.kill()
    proc.wait()


try:
    # ---- 1. 无锁文件：probe = free ----
    state, holder = lockfile.probe()
    assert state == "free", (state, holder)

    # ---- 2. 真进程持锁：acquire 被拒、holder 可读、probe = held ----
    proc = subprocess.Popen(
        [PYTHON, "-c", HOLDER],
        stdout=subprocess.PIPE, text=True, encoding="utf-8", env=ENV,
    )
    try:
        parts = proc.stdout.readline().strip().split()
        assert parts and parts[0] == "HELD", f"持有者没起来：{parts}"
        holder_pid = int(parts[1])
        state, holder = lockfile.probe()
        assert state == "held", (state, holder)
        assert holder and f"pid={holder_pid}" in holder, (holder, holder_pid)
        try:
            lockfile.acquire("update_library")
            raise AssertionError("锁被别人拿着，acquire 必须失败")
        except ProtocolError as e:
            assert e.code == E_LIBRARY_BUSY, e.code
            assert "正被占用" in e.message, e.message
        print(f"[1] 跨进程互斥 OK（holder={holder}）")

        # ---- 3. 杀掉持有者（模拟崩溃）：锁立即释放，无需任何清理 ----
        kill_tree(proc)
        deadline = time.time() + 5
        while time.time() < deadline:
            state, _ = lockfile.probe()
            if state == "free":
                break
            time.sleep(0.1)  # Windows 句柄完全关闭可能有毫秒级滞后
        assert state == "free", f"持有者死后锁必须释放：{state}"
        lockfile.acquire("apply_import")   # 现在必须能拿到
        state, holder = lockfile.probe()
        assert state == "held" and holder, (state, holder)
        print(f"[2] 进程死亡自动释放 OK（本进程接管，holder={holder}）")

        # ---- 4. 同进程续持：acquire 幂等（契约测试一条进程跑多条请求的形态）----
        lockfile.acquire("update_library")  # 不得抛
        print("[3] 同进程续持 OK")
    finally:
        if proc.poll() is None:
            kill_tree(proc)
finally:
    shutil.rmtree(WORK, ignore_errors=True)

print("ALL PASS")
