"""库文件锁（v2）：库变更命令的单写者护栏。

**为什么是命令级而不是进程级**：sidecar 是短进程模型（一命令一进程，
docs/33 §5）——锁随进程生死自动释放，崩溃也一样。常驻服务需要的那套
TTL / 心跳 / 过期回收（beets-web-manager ADR 0005 的机制）在这里整个
不需要：进程退出就是释放，"谁持有"永远等于"哪个进程活着且在干活"。

覆盖的残余风险（docs/41 §3.3，宿主会话槽管不到的）：
  · 双开播放器——两个实例各有一个会话槽，互不知晓；第二实例的变更命令
    在这里被拒（E_LIBRARY_BUSY）。
  · 用户在播放器外面手跑 `beet` 写库——beets 不合作（不拿这把锁），并发写
    由 SQLite 自身的锁兜底；这把锁至少让 status 能把"有人在变更"暴露给 UI。

**锁与持有者信息是两个文件**（Windows 实测的分工）：`library.lock` 只当锁
对象（msvcrt 字节锁会波及同文件其他字节的读——open 直接 PermissionError，
锁多大、偏移在哪都别纠结）；持有者 JSON 写 `library.lock.holder`，探针
读它永远不碰锁字节。holder 文件允许过期：持有者死了锁先释放，probe 报
free 时根本不读它。

平台实现：POSIX `fcntl.flock` / Windows `msvcrt.locking`。锁 API 不可用 →
fail-open（锁只是 advisory，对齐 BWM 的姿态），status 报 `unknown`，命令照常执行。
"""

from __future__ import annotations

import json
import os

from . import cfg
from .protocol import E_LIBRARY_BUSY, ProtocolError

# 拿独占锁的命令（库变更 + 全量导出——导出撞上别人的写事务会 SQLITE_BUSY，
# 不如直接排队/拒绝）。
LOCKED_CMDS = frozenset({"apply_import", "adopt_library", "update_library", "move_items", "dump_items"})

_LOCK_NAME = "library.lock"
_HOLDER_NAME = "library.lock.holder"

# 本进程已持锁时记录当前命令（一命令一进程，但契约测试一条进程跑多条请求）
_held_cmd: str | None = None
# 锁文件建不了（如数据目录只读）：本进程的锁保护不可用。probe 必须如实报
# unknown 而不是 "free"——否则 status 在"没有保护"时谎报"空闲"。
_unavailable = False


def _path() -> str:
    return str(cfg.data_home() / _LOCK_NAME)


def _holder_path() -> str:
    return str(cfg.data_home() / _HOLDER_NAME)


def _open_lock_file() -> int | None:
    flags = os.O_RDWR
    flags |= getattr(os, "O_CREAT", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)  # 锁路径可预测：防符号链接投递
    try:
        return os.open(_path(), flags, 0o600)
    except OSError:
        return None


def _try_lock(fd: int) -> bool:
    """非阻塞独占锁。成功 True；失败/平台不可用 False。"""
    try:
        import fcntl

        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return True
    except ImportError:
        pass
    except OSError:
        return False
    try:
        import msvcrt

        os.lseek(fd, 0, os.SEEK_SET)
        msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
        return True
    except ImportError:
        return False
    except OSError:
        return False


def _unlock_fd(fd: int) -> None:
    try:
        import fcntl

        fcntl.flock(fd, fcntl.LOCK_UN)
        return
    except ImportError:
        pass
    except OSError:
        return
    try:
        import msvcrt

        os.lseek(fd, 0, os.SEEK_SET)
        msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
    except (ImportError, OSError):
        pass


def _write_holder(cmd: str) -> None:
    """把持有者写进旁挂文件（探针据此报 holder）。失败不致命。"""
    try:
        with open(_holder_path(), "w", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps({"pid": os.getpid(), "cmd": cmd}))
    except OSError:
        pass


def _read_holder() -> str | None:
    try:
        with open(_holder_path(), encoding="utf-8") as f:
            data = json.loads(f.read(256) or "{}")
        pid, cmd = data.get("pid"), data.get("cmd")
        if pid or cmd:
            return f"pid={pid or '?'},cmd={cmd or '?'}"
    except (OSError, ValueError):
        pass
    return None


def acquire(cmd: str) -> None:
    """为库变更命令拿锁；被占则抛 ``E_LIBRARY_BUSY``（holder 进 message+extras）。

    本进程已持锁（同一进程连续处理多条命令，契约测试即此形态）→ 只更新
    记录直接放行：同一个进程本来就是同一个写者。
    """
    global _held_cmd
    if _held_cmd is not None:
        _held_cmd = cmd
        _write_holder(cmd)
        return
    fd = _open_lock_file()
    if fd is None:
        global _unavailable
        _unavailable = True
        return  # 锁文件建不了：fail-open，锁是 advisory
    if not _try_lock(fd):
        os.close(fd)
        holder = _read_holder()
        # holder 进 message（Display 原样带给 UI）也进 extras（机器可读）
        who = holder or "另一个进程"
        raise ProtocolError(
            E_LIBRARY_BUSY, cmd,
            f"beets 库正被占用：{who} 正在变更库（可能是另一个播放器实例"
            "或外部 beet 命令）。等它完成后再试。",
            holder=holder,
        )
    _write_holder(cmd)
    _held_cmd = cmd
    # fd 刻意不关：进程生命周期内持锁，退出时 OS 收回（含崩溃）


def probe() -> tuple[str, str | None]:
    """status 用：库锁现在被谁拿着。返回 ``(free|held|unknown, holder)``，绝不抛。"""
    if _held_cmd is not None:
        return "held", _read_holder() or f"pid={os.getpid()},cmd={_held_cmd}"
    if _unavailable:
        return "unknown", None
    try:
        fd = os.open(_path(), os.O_RDWR)
    except FileNotFoundError:
        return "free", None
    except OSError:
        return "unknown", None
    if _try_lock(fd):
        _unlock_fd(fd)
        os.close(fd)
        return "free", None
    os.close(fd)
    return "held", _read_holder()
