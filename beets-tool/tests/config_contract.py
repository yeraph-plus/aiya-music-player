# -*- coding: utf-8 -*-
"""双家隔离契约（docs/45 §4.1 / §6）：DB1 索引家与 DB2 用户库各过各的。

这一批没有任何"功能"，全部是**安全前提**，所以测试也照着门禁写：

  [1] 索引家模板：写盘开关、占位整理根、插件强制参数齐全，且过自检闸
  [2] 负向四连：改 write: yes / 删 duplicate_action / 插件越界 / 整理根指错
      —— 闸必须逐个变红（否则闸是装饰）
  [3] 白名单三元组：写盘插件被结构拒绝，而不是靠"记得别加"
  [4] 环境两态：BEETSDIR 与 AIYA_BEETS_HOME 分家必须拒绝启动；profile 不猜
  [5] 自愈：索引家配置被外部改坏 → 下次启动重渲染回合规
  [6] 两家 status（真子进程）：库路径/配置目录/数据目录/整理根互不串
  [7] 两家锁（真子进程）：索引家持锁不影响用户库，反之亦然
  [8] BEETSDIR 冲突：启动即一条 E_CONFIG 信封，退出码 2

只用临时目录，不碰真实数据目录。
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
TOOL_SRC = os.path.join(TOOL, "src")
VENV_PY = os.path.join(TOOL, ".venv", "Scripts", "python.exe")
if not os.path.exists(VENV_PY):
    VENV_PY = os.path.join(TOOL, ".venv", "bin", "python")
PYTHON = os.environ.get("AIYA_BEETS_PYTHON") or VENV_PY
sys.path.insert(0, TOOL_SRC)

from aiya_beets import cfg, plugins  # noqa: E402

WORK = tempfile.mkdtemp(prefix="aiya-twohomes-")

# 两个家：用户库（workbench）与索引库（index）
HOME_WB = os.path.join(WORK, "beets")
HOME_IDX = os.path.join(WORK, "beets-index")
SCAN_ROOT = os.path.join(WORK, "music")
for d in (HOME_WB, HOME_IDX, SCAN_ROOT):
    os.makedirs(d)


def norm(p: str) -> str:
    """路径比较口径：与 cfg 内部一致（绝对化 + Windows 大小写归一）。"""
    return os.path.normcase(os.path.abspath(p))


def child_env(home: str, profile: str | None = None, **extra) -> dict:
    """子进程环境：只给事实，且清掉父进程里可能残留的 BEETSDIR。"""
    env = dict(os.environ)
    env.pop("BEETSDIR", None)
    env.pop("AIYA_BEETS_DIR", None)
    env["AIYA_BEETS_HOME"] = home
    env["PYTHONPATH"] = TOOL_SRC
    env["PYTHONIOENCODING"] = "utf-8"
    env["PYTHONUTF8"] = "1"
    if profile:
        env["AIYA_BEETS_PROFILE"] = profile
    else:
        env.pop("AIYA_BEETS_PROFILE", None)
    env.update({k: v for k, v in extra.items() if v is not None})
    return env


def run_status(home: str, profile: str | None = None, **extra):
    """跑一次 `status` 子进程，返回 (returncode, 解析后的 JSON 或 None, 原始 stdout)。"""
    proc = subprocess.run(
        [PYTHON, "-m", "aiya_beets", "status"],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        env=child_env(home, profile, **extra),
    )
    line = next((x for x in proc.stdout.splitlines() if x.strip()), "")
    try:
        payload = json.loads(line) if line else None
    except json.JSONDecodeError:
        payload = None
    return proc.returncode, payload, proc.stdout


def must_gate(text: str, home: str, why: str) -> None:
    """断言这份文本过不了闸。"""
    try:
        cfg.verify_config(text, cfg.PROFILE_INDEX, Path(home))
    except cfg.ConfigGateError:
        return
    raise AssertionError(f"闸没红：{why}")


try:
    # ---- 1. 索引家模板 ----
    text = cfg.render_config(Path(HOME_IDX), None, None, cfg.PROFILE_INDEX)
    before = dict(cfg.INDEX_IMPORT_KEYS)
    assert before["write"] == "no" and before["duplicate_action"] == "keep", before
    for line in ("    write: no", "    copy: no", "    move: no",
                 "    duplicate_action: keep", "    incremental: yes"):
        assert line in text, f"索引家模板缺 {line!r}：\n{text}"
    assert "xtractor:" in text and "    auto: no" in text, text
    # xtractor 段的 write 必须是 no（默认值恰好是 yes，不写就等于破前提）
    xtr = text.split("xtractor:", 1)[1]
    assert "write: no" in xtr, text
    # 整理根恒为占位目录，且**不**采用传入的扫描根
    with_root = cfg.render_config(Path(HOME_IDX), SCAN_ROOT, None, cfg.PROFILE_INDEX)
    assert norm(SCAN_ROOT) not in norm(with_root), "索引家不该把扫描根写进 directory"
    assert norm(cfg.scratch_dir(Path(HOME_IDX))) in norm(text), text
    cfg.verify_config(text, cfg.PROFILE_INDEX, Path(HOME_IDX))
    print("[1] 索引家模板 + 自检通过")

    # ---- 2. 负向五连：每条都必须让闸变红 ----
    idx_home = Path(HOME_IDX)
    must_gate(text.replace("    write: no", "    write: yes", 1), HOME_IDX,
              "import.write 被改成 yes")
    must_gate(text.replace("    duplicate_action: keep\n", ""), HOME_IDX,
              "duplicate_action 被删（默认 ask 会吞文件）")
    must_gate(text.replace("plugins: xtractor", "plugins: xtractor embedart"), HOME_IDX,
              "白名单外的插件（embedart 写文件）")
    must_gate(
        text.replace(f"directory: '{cfg.scratch_dir(idx_home)}'", f"directory: '{SCAN_ROOT}'"),
        HOME_IDX, "整理根指向扫描根（库里会存相对路径）",
    )
    must_gate(text.replace("xtractor:\n    auto: no\n    write: no\n", "xtractor:\n    auto: no\n"),
              HOME_IDX, "xtractor.write 缺失（默认 yes 会写回音频文件）")
    # workbench 不设闸：同一个家，用户想怎么写都行
    cfg.verify_config("library: 'x'\n", cfg.PROFILE_WORKBENCH, idx_home)
    print("[2] 负向五连全部变红（闸不是装饰）")

    # ---- 3. 白名单三元组 ----
    assert all(not spec.writes_files for spec in plugins.INDEX_PLUGINS), "索引家不该收写盘插件"
    assert plugins.find_index_plugin("embedart") is None
    fake = plugins.IndexPluginSpec("embedart", forced=(("write", "no"),), writes_files=True)
    real = plugins.INDEX_PLUGINS
    try:
        plugins.INDEX_PLUGINS = real + (fake,)
        bad = text.replace("plugins: xtractor", "plugins: xtractor embedart")
        must_gate(bad, HOME_IDX, "writes_files=True 的插件（结构性拒绝）")
    finally:
        plugins.INDEX_PLUGINS = real
    print("[3] 白名单三元组生效（写盘插件被结构拒绝）")

    # ---- 4. 环境：分家必拒，profile 不猜 ----
    saved = {k: os.environ.get(k) for k in ("BEETSDIR", "AIYA_BEETS_HOME", "AIYA_BEETS_PROFILE")}
    try:
        for k in saved:
            os.environ.pop(k, None)
        # 只有 home → 翻译出 BEETSDIR
        os.environ["AIYA_BEETS_HOME"] = HOME_IDX
        cfg.apply_env()
        assert norm(os.environ["BEETSDIR"]) == norm(HOME_IDX), os.environ["BEETSDIR"]
        # 两者相等 → 放行
        os.environ["BEETSDIR"] = HOME_IDX
        cfg.apply_env()
        # 两者不等 → 拒绝
        os.environ["BEETSDIR"] = HOME_WB
        try:
            cfg.apply_env()
            raise AssertionError("BEETSDIR 与 AIYA_BEETS_HOME 分家必须拒绝启动")
        except cfg.ConfigConflict:
            pass
        # profile：默认 workbench，认识 index，未知值拒绝
        os.environ.pop("AIYA_BEETS_PROFILE", None)
        assert cfg.profile() == cfg.PROFILE_WORKBENCH
        os.environ["AIYA_BEETS_PROFILE"] = "index"
        assert cfg.profile() == cfg.PROFILE_INDEX
        os.environ["AIYA_BEETS_PROFILE"] = "idx"
        try:
            cfg.profile()
            raise AssertionError("未知 profile 必须拒绝（静默回落会写错库）")
        except cfg.ConfigConflict:
            pass
    finally:
        for k, v in saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
    print("[4] 环境两态 OK（分家拒绝 / profile 不猜）")

    # ---- 5. 自愈：外部把索引家配置改坏 → 下次启动重渲染 ----
    path = idx_home / "config.yaml"
    path.write_text(text.replace("    write: no", "    write: yes", 1), encoding="utf-8", newline="\n")
    cfg.ensure_config(idx_home, cfg.PROFILE_INDEX)
    healed = path.read_text(encoding="utf-8")
    cfg.verify_config(healed, cfg.PROFILE_INDEX, idx_home)
    assert "    write: yes" not in healed, "坏配置没有被重写"
    print("[5] 索引家配置自愈 OK")

    # ---- 6. 两家 status：字段互不串 ----
    code_wb, wb, raw_wb = run_status(HOME_WB, "workbench", AIYA_BEETS_SCAN_ROOT=SCAN_ROOT)
    assert code_wb == 0 and wb, f"workbench status 失败：{raw_wb[:300]}"
    code_idx, idx, raw_idx = run_status(HOME_IDX, "index")
    assert code_idx == 0 and idx, f"index status 失败：{raw_idx[:300]}"

    assert norm(wb["libraryPath"]) == norm(os.path.join(HOME_WB, "library.db")), wb["libraryPath"]
    assert norm(idx["libraryPath"]) == norm(os.path.join(HOME_IDX, "library.db")), idx["libraryPath"]
    assert norm(wb["configDir"]) == norm(HOME_WB), wb["configDir"]
    assert norm(idx["configDir"]) == norm(HOME_IDX), idx["configDir"]
    assert norm(wb["dataHome"]) == norm(HOME_WB), wb["dataHome"]
    assert norm(idx["dataHome"]) == norm(HOME_IDX), idx["dataHome"]
    # 整理根：用户库 = 扫描根；索引家 = 占位空目录
    assert norm(wb["libraryDirectory"]) == norm(SCAN_ROOT), wb["libraryDirectory"]
    assert norm(idx["libraryDirectory"]) == norm(cfg.scratch_dir(idx_home)), idx["libraryDirectory"]
    # 索引家只开 xtractor；用户库不带 import 段（三行老形状，用户做主）
    assert idx["plugins"] == ["xtractor"], idx["plugins"]
    wb_cfg = open(os.path.join(HOME_WB, "config.yaml"), encoding="utf-8").read()
    assert "import:" not in wb_cfg, f"用户库的配置不该被塞写盘开关：\n{wb_cfg}"
    print("[6] 两家 status 隔离 OK（库/配置/数据根/整理根四路都不串）")

    # ---- 7. 两家锁：一边被占不影响另一边 ----
    from aiya_beets import lockfile

    HOLDER = (
        "import sys, time;"
        "sys.path.insert(0, r'" + TOOL_SRC + "');"
        "from aiya_beets import lockfile;"
        "lockfile.acquire('apply_import');"
        "import os; print('HELD', os.getpid(), flush=True);"
        "time.sleep(20)"
    )
    proc = subprocess.Popen(
        [PYTHON, "-c", HOLDER],
        stdout=subprocess.PIPE, text=True, encoding="utf-8", env=child_env(HOME_IDX, "index"),
    )
    saved_home = os.environ.get("AIYA_BEETS_HOME")
    try:
        parts = proc.stdout.readline().strip().split()
        assert parts and parts[0] == "HELD", f"持有者没起来：{parts}"
        holder_pid = int(parts[1])
        os.environ["AIYA_BEETS_HOME"] = HOME_IDX
        state, holder = lockfile.probe()
        assert state == "held" and holder and f"pid={holder_pid}" in holder, (state, holder)
        # 关键断言：换成另一个家，同一时刻必须是 free（锁不跨家）
        os.environ["AIYA_BEETS_HOME"] = HOME_WB
        state_wb, holder_wb = lockfile.probe()
        assert state_wb == "free", f"索引家的锁影响了用户库：{state_wb} {holder_wb}"
    finally:
        if saved_home is None:
            os.environ.pop("AIYA_BEETS_HOME", None)
        else:
            os.environ["AIYA_BEETS_HOME"] = saved_home
        if proc.poll() is None:
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                               capture_output=True)
            else:
                proc.kill()
            proc.wait()
    print("[7] 两家锁互不影响 OK")

    # ---- 8. BEETSDIR 冲突：启动即一条 E_CONFIG，退出码 2 ----
    code, payload, raw = run_status(HOME_IDX, "index", BEETSDIR=HOME_WB)
    assert code == 2, f"冲突时必须非零退出并给出信封：code={code} raw={raw[:200]}"
    assert payload and payload.get("ok") is False, raw[:300]
    assert payload["error"]["code"] == "E_CONFIG", payload
    assert "BEETSDIR" in payload["error"]["beets_message"], payload
    print("[8] BEETSDIR 冲突 fail-fast OK")
finally:
    shutil.rmtree(WORK, ignore_errors=True)

print("ALL PASS")
