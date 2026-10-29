"""幂等操作登记簿（v2）：``{dataHome}/ops.jsonl`` 追加式日志。

两个职责（docs/41 §3.2/§6.3）：

1. **重放**：同 ``requestId`` + 同请求内容重入 → 回放上次的事件流（不含
   progress），不重执行；同键不同内容 → ``E_INVALID_ARG``。
   **只重放 done**：failed / cancelled 不重放——用户修完冲突的重跑必须
   真执行，不能拿到上次的失败（A3.5 语义）。
2. **审计**：所有库变更操作的时间线（who/when/what/outcome），未来
   P1-5 任务注册表的数据层前身。

为什么单实例下重放几乎不触发：宿主的会话槽已挡重复提交（第二个
``spawn_session`` 直接 ``Busy``）。重放防的是协议层残余——双开播放器、
未来宿主加自动重试。**审计才是每天都在发生的事**：每次 apply/adopt/
update/move/dump 都留一行，出问题有账可查。

并发模型：一命令一进程（docs/33 §5），begin/finish 都在主循环线程；
jsonl 追加 + 读时取最新（同 requestId 的 running 残留被后续覆盖）。
dataHome 不可写 → 全部 fail-open（丢幂等不丢功能），写 stderr 留痕。
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
import time

from . import cfg
from .protocol import E_INVALID_ARG, ProtocolError

# 变更命令 → 终态事件名（ops 只服务这四个；dump 是读，不进登记簿）
TERMINAL_OF = {
    "apply_import": "applyDone",
    "adopt_library": "adoptDone",
    "update_library": "updateDone",
    "move_items": "moveDone",
}

# 会话名前缀（与 schema 的 Session pattern 一字不差）
SESSION_PREFIX = {
    "apply_import": "app",
    "adopt_library": "adopt",
    "update_library": "update",
    "move_items": "move",
}

_MAX_ENTRY_BYTES = 2 * 1024 * 1024  # 超过就修剪
_KEEP_ENTRIES = 200                 # 修剪后保留的最新条数


def _path():
    return cfg.data_home() / "ops.jsonl"


def payload_hash(req: dict) -> str:
    """请求内容指纹：去掉 id / cmd / requestId / clientProtocol 后的规范 JSON。

    重放的正确性靠它：同键不同内容必须判冲突，不能把上一次的结果错发给
    不同的请求。
    """
    body = {k: v for k, v in req.items() if k not in ("id", "cmd", "requestId", "clientProtocol")}
    return hashlib.sha256(
        json.dumps(body, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()


def _load() -> list[dict]:
    """读登记簿，跳过撕裂/损坏行。最新在后。"""
    try:
        with open(_path(), encoding="utf-8") as f:
            lines = f.readlines()
    except OSError:
        return []
    out = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue  # 崩溃时的撕裂尾行
        if isinstance(rec, dict):
            out.append(rec)
    return out


def _append(rec: dict) -> bool:
    try:
        cfg.data_home().mkdir(parents=True, exist_ok=True)
        with open(_path(), "a", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        return True
    except OSError as e:
        print(f"[aiya-beets] ops 登记簿写入失败（幂等停用）：{e}", file=sys.stderr)
        return False


def _prune() -> None:
    """超过上限时重写保留最新 N 条。临时文件 + 原子替换，崩溃不丢账。"""
    path = _path()
    try:
        if os.path.getsize(path) <= _MAX_ENTRY_BYTES:
            return
        records = _load()[-_KEEP_ENTRIES:]
        tmp = str(path) + ".tmp"
        with open(tmp, "w", encoding="utf-8", newline="\n") as f:
            for rec in records:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        os.replace(tmp, path)
    except OSError:
        pass  # 修剪是锦上添花


def lookup(request_id: str, cmd: str, phash: str) -> list[dict] | None:
    """查 done 记录：命中返回可重放的事件流；同键不同内容抛 E_INVALID_ARG。

    纯读，不加锁不改文件——重放发生在拿库锁之前（重放不触碰库）。
    """
    for rec in reversed(_load()):
        if rec.get("requestId") != request_id:
            continue
        if rec.get("cmd") != cmd or rec.get("payloadHash") != phash:
            raise ProtocolError(
                E_INVALID_ARG, cmd,
                f"requestId {request_id!r} 已被不同内容的 {rec.get('cmd')} 请求使用："
                "换一个 requestId（宿主每次调用都应生成新键）",
            )
        if rec.get("status") == "done":
            events = rec.get("events")
            if isinstance(events, list) and events:
                return events
        return None  # running/failed/cancelled 残留：不重放，重跑真执行
    return None


def register(request_id: str, cmd: str, phash: str) -> "Recorder | None":
    """登记 running 并返回事件收集器；登记簿不可用时返回 None（fail-open）。"""
    ok = _append({
        "requestId": request_id, "cmd": cmd, "payloadHash": phash,
        "pid": os.getpid(), "ts": int(time.time()), "status": "running",
    })
    _prune()
    return Recorder(request_id, cmd, phash) if ok else None


class Recorder:
    """挂在 runtime.emit 上收集本次操作的事件流（progress 不记）。

    终态事件（``*Done`` / ``cancelled``）出现即落账 done / cancelled；
    handler 抛异常走 :meth:`finish_failed`（serve 的兜底分支调用）。
    """

    def __init__(self, request_id: str, cmd: str, phash: str):
        self.request_id = request_id
        self.cmd = cmd
        self.phash = phash
        self.terminal = TERMINAL_OF.get(cmd)
        self._events: list[dict] = []
        self._closed = False

    def on_event(self, evt: dict) -> None:
        if self._closed:
            return
        t = evt.get("type")
        if t == "progress" or t == "cancelAck":
            return
        self._events.append(evt)
        if t == self.terminal:
            self._finish("done")
        elif t == "cancelled":
            self._finish("cancelled")

    def finish_failed(self) -> None:
        self._finish("failed")

    def _finish(self, status: str) -> None:
        if self._closed:
            return  # 第一个终态定账，后来的（异常后的兜底）忽略
        self._closed = True
        _append({
            "requestId": self.request_id, "cmd": self.cmd,
            "payloadHash": self.phash, "pid": os.getpid(),
            "ts": int(time.time()), "status": status,
            "events": self._events,
        })
