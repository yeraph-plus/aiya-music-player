"""插件市场（白名单版，docs/40 §3 拍板收敛）：目录 + 安装。

## 拍板范围（2026-10-28 用户确认）

不做全量市场。**只收录调研过的有用插件**（MARKET_CATALOG 白名单），
项目内只集成"把插件装进当前运行环境"的 pip 命令——启停仍走 config.yaml
（cfg.render_config，播放器不知道 YAML 语法，docs/33 P1）。

## 环境兼容（uv / venv / 系统 Python）

安装目标恒为 **serve 进程自己的解释器**（`sys.executable`）：
A 轨道 venv、uv 管理的环境、系统 Python——不管 beets 当初怎么装的，
"能跑 serve 的环境"就是"插件该去的环境"，不做任何二次猜测。

pip 可用性按序探测（uv 建的 venv 默认**不带 pip**，这是实测行为）：
  1. `sys.executable -m pip --version` 成功 → 用它；
  2. 失败但 PATH 上有 `uv` → `uv pip install --python <sys.executable>`；
  3. 都没有 → 明确报错（B 轨道离线套件包属这一类：_pth 固定 site-packages
     且无 pip，装 PyPI 插件需要走 A 轨道重装）。

输出走行帧事件（progress / done），与 maintain.py 同一套终端事件约定
（`done` 是 TERMINAL_EVENTS 之一，宿主 run_session 直接可用）。
"""

from __future__ import annotations

import os
import queue
import shutil
import subprocess
import sys
import threading

from .. import plugins

# 与 install.py 同一坑：本进程无控制台，每个子进程都要单独压黑窗
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


def catalog_payload() -> list[dict]:
    """市场目录：白名单逐项给三态 + 来源档位。"""
    return plugins.market_report()


def _pip_prefix(python: str) -> tuple[list[str], str] | None:
    """探测 `python -m pip`。返回命令前缀 + 名称，不可用返回 None。"""
    try:
        probe = subprocess.run(
            [python, "-m", "pip", "--version"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            **_NO_WINDOW,
        )
    except OSError:
        return None
    if probe.returncode == 0:
        return [python, "-m", "pip"], "pip"
    return None


def resolve_installer() -> tuple[list[str], str] | tuple[None, str]:
    """按序探测安装器：venv pip → uv pip。"""
    python = os.environ.get("PYTHON", sys.executable)
    hit = _pip_prefix(python)
    if hit is not None:
        return hit
    uv = shutil.which("uv")
    if uv:
        # uv 建的 venv 不带 pip 是常态：uv pip 直接操作目标环境，不需要 pip 模块
        return [uv, "pip", "install", "--python", python], "uv pip"
    return None, (
        "当前 Python 环境没有 pip，PATH 上也没有 uv，无法在线安装插件。"
        "离线套件包（bundle）轨道请改用安装器（installer）轨道，或在打包时预置 wheel。"
    )


def install_plugin(req_id: int, name: str, emit, cancel_gen, gen: int) -> None:
    """安装白名单里的一个插件（事件流应答，与 maintain 同款）。

    事件：`pluginProgress`（安装器逐行输出）→ `pluginDone`（复检通过，
    带完整目录报告）；失败直接抛 `ProtocolError`（serve 统一转
    sessionFailed 信封），取消走 `cancelled`。
    """
    from . import protocol

    session = f"plug_{req_id:02d}"
    spec = plugins.find_market_spec(name)
    if spec is None:
        raise protocol.ProtocolError(
            protocol.E_INVALID_ARG, "pluginInstall", f"不在插件目录里: {name!r}"
        )
    if spec.pypi is None:
        # builtin 档：随 beets 内置，"安装"没有意义，UI 对它只给开关
        raise protocol.ProtocolError(
            protocol.E_INVALID_ARG, "pluginInstall", f"{name} 随 beets 内置，无需安装"
        )

    installer, why = resolve_installer()
    if installer is None:
        raise protocol.ProtocolError(protocol.E_CONFIG, "pluginInstall", why)

    cmd = [*installer, "install", "--disable-pip-version-check", spec.pypi]
    emit({"type": "pluginProgress", "session": session, "stage": "pluginInstall",
          "message": f"安装 {spec.pypi}"})
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
            **_NO_WINDOW,
        )
    except OSError as e:
        raise protocol.ProtocolError(
            protocol.E_INTERNAL, "pluginInstall", f"启动安装器失败: {e}"
        ) from e

    # v2 积弊修复：取消必须能杀掉 pip。旧实现阻塞在 `for line in proc.stdout`
    # 上，cancel 检查排在 proc.wait() 之后——卡死的 pip 让"取消"永远无效。
    # 现在输出走泵线程 + 队列，主循环 0.5s 轮询一次取消代数，到点 kill+wait。
    assert proc.stdout is not None
    lines: "queue.Queue[str | None]" = queue.Queue()

    def _pump() -> None:
        try:
            for line in proc.stdout:
                lines.put(line)
        finally:
            lines.put(None)  # stdout 关闭 = 进程要退出了

    threading.Thread(target=_pump, name="pip-recv", daemon=True).start()
    while True:
        try:
            line = lines.get(timeout=0.5)
        except queue.Empty:
            if cancel_gen() != gen:
                proc.kill()
                proc.wait()
                emit({"type": "cancelled", "session": session, "stage": "pluginInstall"})
                return
            continue
        if line is None:
            break
        line = line.rstrip()
        if line:
            emit({"type": "pluginProgress", "session": session, "stage": "pluginInstall",
                  "message": line})
    code = proc.wait()
    if cancel_gen() != gen:
        emit({"type": "cancelled", "session": session, "stage": "pluginInstall"})
        return
    if code != 0:
        raise protocol.ProtocolError(
            protocol.E_BEETS_ERROR, "pluginInstall",
            f"安装退出码 {code}（详见上方输出）",
        )

    # 装完必须复检三态：pip 退出码 0 不等于插件可用（依赖缺、包名与插件名不一致）
    report = plugins.market_report()
    state = next((item["state"] for item in report if item["name"] == name), "unavailable")
    if state != plugins.READY:
        missing = next((item["missing"] for item in report if item["name"] == name), [])
        raise protocol.ProtocolError(
            protocol.E_BEETS_ERROR, "pluginInstall",
            f"安装完成但插件未就绪（{state}），缺：{', '.join(missing) or '未知'}",
        )

    emit({"type": "pluginDone", "session": session, "stage": "pluginInstall",
          "name": name, "report": report})
