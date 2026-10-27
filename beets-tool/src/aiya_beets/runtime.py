"""协议运行时：编码钉死、stdout 写锁、取消代数、事件输出。

这三样都是**进程级全局状态**：stdout 只有一条、取消代数只有一份。
单独成模块是为了让 `cmds/*` 能 import 而不依赖 server（否则循环导入）。
"""

from __future__ import annotations

import json
import sys
import threading

# 取消代数（generation），不是布尔标志。
#
# 为什么不能用一个 Event：宿主发 cancel 时可能"请求行已进管道、主循环还没派发"
# —— 旧实现每次派发前 CANCEL.clear()，于是刚置的取消被下一条请求抹掉，取消
# 静默失效（表现为点了取消还在写标签）。改成自增代数后：
#   * 请求入队时记下当时的代数；派发时比较"当前代数是否变过"，
#     变过就说明自己在排队期间被取消过 → 直接跳过；
#   * 工作循环里同样比较代数，变过就收尾发 cancelled。
_CANCEL_GEN = 0
_CANCEL_LOCK = threading.Lock()


def bump_cancel() -> int:
    global _CANCEL_GEN
    with _CANCEL_LOCK:
        _CANCEL_GEN += 1
        return _CANCEL_GEN


def cancel_gen() -> int:
    with _CANCEL_LOCK:
        return _CANCEL_GEN


# stdout 写锁：reader 线程的 cancelAck 与主线程的事件流/响应并发写 stdout，
# 无锁时两行可能交错破坏行帧协议
_OUT_LOCK = threading.Lock()


def force_utf8_stdio() -> None:
    """把 stdin/stdout 钉成 UTF-8（行协议的唯一编码）。

    为什么必须显式设置：Windows 上 Python 对管道用**本地编码**（简中 = cp936），
    `json.dumps(..., ensure_ascii=False)` 出来的中文会以 GBK 字节写出，宿主按
    UTF-8 读 → 非法字节 → 旧 reader 直接 break，整个会话无声死掉（用户看到的是
    "导入没反应"）。ensure_ascii=True 能绕过，但会把路径/标题全变 \\uXXXX，
    日志与断点调试都失去可读性，所以选择在协议层钉编码。
    """
    for stream, kwargs in (
        (sys.stdin, {"errors": "replace"}),
        (sys.stdout, {"errors": "replace", "newline": "\n"}),
    ):
        try:
            stream.reconfigure(encoding="utf-8", **kwargs)
        except (AttributeError, ValueError):  # 非标准流（测试替身/重定向）忽略
            pass


def write_line(payload: dict) -> None:
    """写一条完整消息（应答信封或事件）。所有 stdout 写入都走这里 + 同一把锁。"""
    text = json.dumps(payload, ensure_ascii=False) + "\n"
    with _OUT_LOCK:
        sys.stdout.write(text)
        sys.stdout.flush()


def emit(evt: dict) -> None:
    """流式事件（进度/专辑/结果）——与响应不同，无 id，靠 type 区分。"""
    write_line(evt)
