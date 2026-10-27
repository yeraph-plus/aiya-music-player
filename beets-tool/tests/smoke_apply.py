# -*- coding: utf-8 -*-
"""B3 apply_import 冒烟：只写标签 / 复制 / 移动 / 目标冲突 / 取消。

隔离运行：独立临时 BEETSDIR + 独立库目录，不碰 dev 库和真实 Music 目录。
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)                 # beets-tool/
REPO = os.path.dirname(TOOL)                 # 仓库根
# 工具按 `python -m aiya_beets` 起（源码树跑，不需要先 pip install）
TOOL_SRC = os.path.join(TOOL, "src")
# 解释器：优先 $AIYA_BEETS_PYTHON；否则用仓库根的 dev venv（tools/ 里，不入库）
PYTHON = os.environ.get("AIYA_BEETS_PYTHON") or os.path.join(
    TOOL, ".venv", "Scripts", "python.exe")
TEST_MUSIC = os.environ.get("AIYA_TEST_MUSIC") or r"D:\MUSIC_Player\test-music"
SRC_WAV = sorted(
    os.path.join(TEST_MUSIC, f)
    for f in os.listdir(TEST_MUSIC) if f.lower().endswith(".wav")
)
assert len(SRC_WAV) == 3, SRC_WAV

WORK = tempfile.mkdtemp(prefix="aiya-b3-")
BEETSDIR = os.path.join(WORK, "beetsdir")
LIBDIR = os.path.join(WORK, "lib")
os.makedirs(BEETSDIR)
os.makedirs(LIBDIR)
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    # copy: no —— 防止 beets 导入默认复制行为干扰（我们用 sidecar 手动控制动作）
    f.write(f"directory: {LIBDIR}\nimport:\n    copy: no\n")

ENV = dict(os.environ, BEETSDIR=BEETSDIR, PYTHONIOENCODING="utf-8",
           PYTHONPATH=TOOL_SRC)


def run_apply(decisions, timeout=120):
    """一次性 sidecar 进程跑 apply_import，返回 (events, returncode)。"""
    proc = subprocess.Popen(
        [PYTHON, "-m", "aiya_beets"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        text=True, encoding="utf-8", env=ENV,
    )
    req = json.dumps({"id": 7, "cmd": "apply_import", "decisions": decisions}, ensure_ascii=False)
    try:
        out, _ = proc.communicate(req + "\n", timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        out, _ = proc.communicate()
        raise
    events = []
    for line in out.splitlines():
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            # beets 偶发往 stdout 打迁移/备份提示（如"Created database backup"），
            # Rust reader 同样容错忽略；这里跳过不视为失败
            print("skip non-JSON:", line[:120])
            continue
        if "type" in obj:
            events.append(obj)
    return events, proc.returncode


def decision(action, paths, album_name, artist="测试艺人", title_prefix="曲目"):
    # 标题要带场景前缀：beets 默认 singleton 模板 Non-Album/$artist/$title
    # 不含专辑名，不同场景同标题会在库内撞目标路径
    mapping = {
        p: {"title": f"{title_prefix} {i:02d}", "artist": artist, "track": i, "disc": 1}
        for i, p in enumerate(paths, 1)
    }
    return {
        "album": "alb_test",
        "action": action,
        "paths": paths,
        "candidate": {
            "album": {"album": album_name, "albumartist": artist, "year": 2026, "genre": "测试流派"},
            "mapping": mapping,
        },
    }


def beets_items():
    sys.path.insert(0, os.path.join(TOOL, ".venv", "Lib", "site-packages"))
    os.environ["BEETSDIR"] = BEETSDIR
    import beets
    from beets.library import Library
    # 与 sidecar _open_library 同款：beets 2.14 的 Library(path) 默认绑系统
    # Music 目录做路径展开 context，必须显式传 config 的 directory
    lib = Library(
        beets.config["library"].as_filename(),
        beets.config["directory"].as_filename(),
    )
    return lib, list(lib.items())


def fresh_src(dirname):
    d = os.path.join(WORK, dirname)
    os.makedirs(d)
    out = []
    for p in SRC_WAV:
        dst = os.path.join(d, os.path.basename(p))
        shutil.copy2(p, dst)
        out.append(dst)
    return out


# ---- 场景 1：apply 只写标签，文件不移动 ----
src1 = fresh_src("src1")
events, rc = run_apply([decision("apply", src1, "冒烟A", title_prefix="A曲目")])
types = [e["type"] for e in events]
assert "applied" in types and "applyDone" in types, types
assert rc == 0, rc
from mutagen.wave import WAVE
tags = WAVE(src1[0])
assert str(tags["TIT2"]) == "A曲目 01", tags["TIT2"]
lib, items = beets_items()
assert len(items) == 3, len(items)
assert all(it.path.decode("utf-8", "replace").lower().startswith(os.path.join(WORK, "src1").lower()) for it in items), "文件应留在原地"
print("[1] 只写标签 OK（文件未移动）")

# ---- 场景 2：apply_copy —— 复制进库，源保留 ----
events, rc = run_apply([decision("apply_copy", src1, "冒烟A", title_prefix="A曲目")])
applied = [e for e in events if e["type"] == "applied"][0]
assert applied["moved"] is True, applied
assert applied["written"] == 3 and applied["failed"] == [], applied
assert os.path.exists(src1[0]), "copy 后源文件必须保留"
lib, items = beets_items()
assert len(items) == 3, len(items)
new_paths = [it.path.decode("utf-8", "replace") for it in items]
assert all(p.lower().startswith(LIBDIR.lower()) for p in new_paths), new_paths
assert all(os.path.exists(p) for p in new_paths), "库内文件应存在"
print("[2] 复制进库 OK:", new_paths[0])

# ---- 场景 3：目标冲突 —— 不覆盖不改名 ----
src3 = fresh_src("src3")
# 先 copy 一次拿到真实 dest
events, rc = run_apply([decision("apply_copy", src3, "冒烟C", title_prefix="C曲目")])
lib, items = beets_items()
dests = [it.path.decode("utf-8", "replace") for it in items if "冒烟C" in it.album]
assert len(dests) == 3, dests
# 删掉 beets item + 库内文件，在 dest 放一个哨兵文件
for it in list(items):
    if "冒烟C" in it.album:
        it.remove()
for d in dests:
    os.remove(d)
    with open(d, "w", encoding="utf-8") as f:
        f.write("SENTINEL-DO-NOT-OVERWRITE")
events, rc = run_apply([decision("apply_copy", src3, "冒烟C", title_prefix="C曲目")])
applied = [e for e in events if e["type"] == "applied"][0]
conflicts = [f for f in applied["failed"] if f.get("code") == "conflict"]
assert len(conflicts) == 3, (applied, conflicts)
assert all("SENTINEL" not in f["message"] or "目标已存在" in f["message"] for f in conflicts), conflicts
for d in dests:
    with open(d, encoding="utf-8") as f:
        assert f.read() == "SENTINEL-DO-NOT-OVERWRITE", f"哨兵被覆盖: {d}"
assert os.path.exists(src3[0]), "冲突时源文件不应被动"
print("[3] 目标冲突不覆盖 OK")

# ---- 场景 4：apply_move —— 移动进库，源消失 ----
src4 = fresh_src("src4")
events, rc = run_apply([decision("apply_move", src4, "冒烟D", title_prefix="D曲目")])
applied = [e for e in events if e["type"] == "applied"][0]
assert applied["failed"] == [], applied
assert not os.path.exists(src4[0]), "move 后源文件应消失"
lib, items = beets_items()
d_paths = [it.path.decode("utf-8", "replace") for it in items if "冒烟D" in it.album]
assert len(d_paths) == 3 and all(os.path.exists(p) for p in d_paths), d_paths
assert all(os.listdir(os.path.dirname(p)) for p in d_paths), "库内文件应存在"
# 源目录应被 prune（beets move 清理空目录）
assert not os.path.exists(src4[0]), ""
print("[4] 移动进库 OK")

# ---- 场景 5：取消 —— 覆盖"已入队未派发"与"执行中"两个窗口 ----
# 12 组决策（36 个文件）让 apply 有足够的执行时间；cancel 紧跟请求行写入，
# 命中哪个窗口取决于线程调度，但**两个窗口都必须收尾成 cancelled**。
# 旧断言 `"cancelled" in ev or "applyDone" in ev` 恰好把 P1-7（取消被
# CANCEL.clear() 抹掉、全量照跑）放过去，所以这里改成排他断言。
src5_groups = [fresh_src(f"src5_{i:02d}") for i in range(12)]
decisions5 = [
    decision("apply", paths, f"冒烟E{i:02d}", title_prefix=f"E{i:02d}曲目")
    for i, paths in enumerate(src5_groups)
]
proc = subprocess.Popen(
    [PYTHON, "-m", "aiya_beets"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    text=True, encoding="utf-8", env=ENV,
)
req = json.dumps({"id": 9, "cmd": "apply_import", "decisions": decisions5}, ensure_ascii=False)
proc.stdin.write(req + "\n")
proc.stdin.write(json.dumps({"id": 10, "cmd": "cancel"}) + "\n")
proc.stdin.flush()
proc.stdin.close()
out, _ = proc.communicate(timeout=120)
ev_types = []
for line in out.splitlines():
    if line.strip():
        try:
            obj = json.loads(line)
            if "type" in obj:
                ev_types.append(obj["type"])
        except json.JSONDecodeError:
            pass
assert "cancelAck" in ev_types, ev_types
terminal = [t for t in ev_types if t in ("cancelled", "applyDone")]
assert len(terminal) == 1, f"终止事件必须恰好一个，实际 {terminal}（{ev_types}）"
assert terminal[0] == "cancelled", f"取消后不能出现 applyDone（全量照跑=P1-7 回归）: {ev_types}"
assert proc.returncode == 0, proc.returncode
print("[5] 取消 OK:", ev_types)

# ---- 清理 ----
shutil.rmtree(WORK, ignore_errors=True)
print("ALL PASS")
