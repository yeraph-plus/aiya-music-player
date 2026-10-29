"""stdio 服务端：请求分派与主循环。

这一层只做三件事：读一行、派发、写一行。业务全在 `cmds/`，
环境自检在 `env.py`。它不 import beets（除非走到对应命令），
所以 `aiya-beets --help` 在没装 beets 的机器上也该能跑。
"""

from __future__ import annotations

import errno
import json
import queue
import sys
import threading
import traceback

from . import cfg, env, lockfile, ops, protocol
from .cmds import dump, importer, maintain, market
from .runtime import (
    bump_cancel,
    cancel_gen,
    emit,
    force_utf8_stdio,
    open_protocol_stream,
    set_op_recorder,
    write_line,
)


def handle(req: dict, gen: int) -> dict | None:
    """返回 None 表示已用事件流自行应答（preview/apply/dump/维护模式）。

    `gen` = 该请求入队时的取消代数：主循环据此判断"排队期间是否被取消过"。
    """
    cmd = req.get("cmd")
    req_id = int(req.get("id") or 0)

    # ---- v2：幂等重放 / 登记（在锁与执行之前）----
    # 重放不触碰库、不加锁：它只是把上次的账本再念一遍。
    # 次序必须是 重放 → 锁 → 登记 running：被锁拒绝的请求不该在登记簿里
    # 留下 running 残留。
    terminal = ops.TERMINAL_OF.get(cmd)
    request_id = req.get("requestId")
    if terminal is not None and request_id:
        request_id = str(request_id)
        phash = ops.payload_hash(req)
        replay = ops.lookup(request_id, cmd, phash)  # 同键不同内容在此抛 E_INVALID_ARG
        if replay is not None:
            # session 字段改写成当前会话名：客户端按会话名把事件归到本次请求
            session = f"{ops.SESSION_PREFIX[cmd]}_{req_id:02d}"
            for evt in replay:
                emit({**evt, "session": session})
            return None
        if cmd in lockfile.LOCKED_CMDS:
            lockfile.acquire(cmd)  # 被占 → E_LIBRARY_BUSY（holder 进 extras）
        set_op_recorder(ops.register(request_id, cmd, phash))  # 不可写时为 None
    elif cmd in lockfile.LOCKED_CMDS:
        # 无幂等键的变更命令照旧拿锁——单写者护栏不依赖 requestId
        lockfile.acquire(cmd)

    # ---- 管理 ----
    if cmd == "status":
        return protocol.ok_response(req_id, env.status_payload())
    if cmd == "ping":
        return protocol.ok_response(
            req_id, {"protocolVersion": protocol.PROTOCOL_VERSION}
        )
    if cmd == "capabilities":
        return protocol.ok_response(req_id, env.capabilities_payload())

    # ---- 导入 ----
    if cmd == "preview_import":
        paths = req.get("paths") or []
        if not paths:
            return protocol.error_response(
                req_id, protocol.E_INVALID_ARG, "preview", "paths 为空"
            )
        importer.preview_import(req_id, [str(p) for p in paths], req.get("options") or {}, gen)
        return None
    if cmd == "apply_import":
        decisions = req.get("decisions") or []
        if not decisions:
            return protocol.error_response(
                req_id, protocol.E_INVALID_ARG, "apply", "decisions 为空"
            )
        importer.apply_import(req_id, decisions, bool(req.get("resume")), gen)
        return None

    # ---- 对账 ----
    if cmd == "dump_items":
        dump.dump_items(req_id, gen)
        return None

    # ---- 曲库维护 ----
    if cmd == "adopt_library":
        paths = req.get("paths") or []
        if not paths:
            return protocol.error_response(
                req_id, protocol.E_INVALID_ARG, "adopt", "paths 为空"
            )
        maintain.adopt_library(req_id, [str(p) for p in paths], gen)
        return None
    if cmd == "update_library":
        maintain.update_library(req_id, bool(req.get("pretend")), gen)
        return None
    if cmd == "find_duplicates":
        maintain.find_duplicates(req_id, req.get("keys") or [], gen)
        return None
    if cmd == "move_items":
        maintain.move_items(req_id, str(req.get("directory") or ""),
                            bool(req.get("pretend")), gen)
        return None

    # ---- 插件市场（docs/40 §3 拍板白名单版；协议纯加法） ----
    if cmd == "plugin_catalog":
        return protocol.ok_response(req_id, {"catalog": market.catalog_payload()})
    if cmd == "plugin_install":
        name = str(req.get("name") or "")
        if not name:
            return protocol.error_response(
                req_id, protocol.E_INVALID_ARG, "pluginInstall", "name 为空"
            )
        market.install_plugin(req_id, name, emit, cancel_gen, gen)
        return None

    return protocol.error_response(
        req_id, protocol.E_UNKNOWN_CMD, "dispatch", f"unknown cmd: {cmd!r}"
    )


# 异常归类（docs/33 §4.5）：宿主对这两类的 UX 完全不同——
#   E_BEETS_ERROR  展示 beets 原文，用户可能自己能修（权限、库损坏、缺 ffmpeg）
#   E_INTERNAL     我们自己的 bug，附 trace，让用户去反馈
# 按异常模块归类而不是按消息文本：文本会变，模块名不会。
_BEETS_MODULES = ("beets", "beetsplug", "mediafile", "sqlite3")
# 数据目录不可写（只读盘 / 被占用 / 无权限）：用户自己能修，不是 bug
_ACCESS_ERRNOS = {errno.EACCES, errno.EPERM, errno.EROFS}


def classify(exc: BaseException) -> str:
    module = type(exc).__module__.split(".")[0]
    if module in _BEETS_MODULES:
        return protocol.E_BEETS_ERROR
    if isinstance(exc, OSError) and exc.errno in _ACCESS_ERRNOS:
        return protocol.E_CONFIG
    return protocol.E_INTERNAL


def _op_fail() -> None:
    """命令异常收尾：把登记簿里的 running 记成 failed（重试才会真执行）。

    正常终态由 ops.Recorder 在事件出口自动落账；异常路径没有终态事件
    （错误信封走 write_line，不经 emit），所以在这里补。
    """
    from .runtime import op_recorder

    rec = op_recorder()
    if rec is not None:
        rec.finish_failed()


def _not_installed(message: str) -> None:
    """beets 不可用时的应答：让宿主给出安装指引，而不是"静默失败"。"""
    write_line(
        protocol.error_response(0, protocol.E_NOT_INSTALLED, "import", message)
    )


def serve() -> int:
    """主循环。返回进程退出码（0 = 正常收尾）。

    编码必须最先钉死：后面任何一行输出都要走 UTF-8，包括"没装 beets"那条信封。
    """
    force_utf8_stdio()
    # 紧接着独占 stdout：下面任何 import/迁移/第三方 print 都不许进协议流
    open_protocol_stream()

    # 首次启动就是"准备环境"的时机。失败不阻断——没配置时 beets 用内置默认
    # 也能跑，只是插件全关，比"完全不可用"好得多。
    try:
        cfg.ensure_config()
    except OSError as e:
        print(f"[aiya-beets] 生成默认配置失败：{e}", file=sys.stderr)

    try:
        import beets  # noqa: F401
        import beets.ui  # noqa: F401
    except ImportError:
        _not_installed("this interpreter has no beets installed")
        return 0
    except SystemExit:
        _not_installed("beets import raised SystemExit")
        return 0

    # cancel 必须在工作循环运行时也能被接收：后台线程持续读 stdin。
    # cancel 在 reader 线程**立即递增取消代数**——此前 cancel 行与其他请求
    # 一样进队列，而主循环正同步执行 preview/apply（网络匹配+写文件），
    # req_q.get() 根本轮不到它，取消永远看不到。
    # 队列元素是 (行, 入队时的取消代数)：排队期间被取消过的请求在派发时
    # 就能识别出来（旧实现的 CANCEL.clear() 会把这个取消抹掉）。
    req_q: queue.Queue = queue.Queue()

    def _read_stdin() -> None:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                req = json.loads(line)
            except json.JSONDecodeError:
                req = None
            if isinstance(req, dict) and req.get("cmd") == "cancel":
                bump_cancel()
                emit({"type": "cancelAck"})
                continue
            req_q.put((line, cancel_gen()))
        req_q.put(None)  # EOF

    threading.Thread(target=_read_stdin, name="stdin-reader", daemon=True).start()

    while True:
        item = req_q.get()
        if item is None:
            break
        line, gen = item
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            resp = protocol.error_response(0, protocol.E_BAD_JSON, "parse", str(e))
            req_id = 0
        else:
            if isinstance(req, dict) and req.get("cmd") == "cancel":
                # 兜底分支（正常路径已在 reader 线程旁路处理）
                bump_cancel()
                emit({"type": "cancelAck"})
                continue
            if not isinstance(req, dict):
                # 合法 JSON 但不是对象：`[1,2]` / `"x"` / `42` 都到这里。
                # 不先挡住的话 `req.get` 会抛 AttributeError 被归类成 E_INTERNAL，
                # 而它其实是调用方拼错了请求——该报参数错。
                write_line(protocol.error_response(
                    0, protocol.E_BAD_JSON, "parse", f"请求必须是 JSON 对象：{type(req).__name__}",
                ))
                continue
            req_id = req.get("id", 0)
            try:
                req_id = int(req_id or 0)
            except (TypeError, ValueError):
                # 线上 id 恒为整数：宿主按它配请求与应答，乱值归零比透传安全
                req_id = 0
            try:
                protocol.check_client(req)
                resp = handle(req, gen)
            except protocol.ProtocolError as e:
                # 业务判定失败（版本不符、根不一致…）：原样带码回，不当 bug
                _op_fail()
                resp = protocol.error_response(
                    req_id, e.code, e.stage, e.message, **e.extra
                )
            except Exception as e:  # noqa: BLE001 —— 顶层兜底，任何异常都要回信封
                _op_fail()
                code = classify(e)
                resp = protocol.error_response(
                    req_id, code, "handle", f"{type(e).__name__}: {e}",
                    trace=traceback.format_exc(limit=4),
                )
            finally:
                set_op_recorder(None)
        # None = 命令已用事件流自行应答（preview_import 等），不再发 id 响应
        if resp is not None:
            payload = {"id": req_id, **{k: v for k, v in resp.items() if k != "id"}}
            if payload.get("ok") is False:
                # 会话模式：错误信封也是终止事件（P2：宿主 reader 只认带 type
                # 的终止事件，错误信封曾让会话卡死在等 EOF、槽位占死）
                payload["type"] = "sessionFailed"
            write_line(payload)
    return 0
