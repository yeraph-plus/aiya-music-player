# -*- coding: utf-8 -*-
"""批 D 曲库维护四件套冒烟：adopt / update / duplicates / move。

隔离运行：独立临时 BEETSDIR + 独立库目录，不碰 dev 库和真实 Music 目录。

跑法： python tools/smoke_maint.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

REPO = r"D:\MUSIC_Player\AIYA_MUSIC_TAURI"
SIDECAR = os.path.join(REPO, "sidecar", "beets_sidecar.py")
PYTHON = os.path.join(REPO, "tools", "beets-venv", "Scripts", "python.exe")
SRC_WAV = sorted(
    os.path.join(r"D:\MUSIC_Player\test-music", f)
    for f in os.listdir(r"D:\MUSIC_Player\test-music") if f.lower().endswith(".wav")
)
assert len(SRC_WAV) == 3, SRC_WAV

WORK = tempfile.mkdtemp(prefix="aiya-bd-")
BEETSDIR = os.path.join(WORK, "beetsdir")
LIBDIR = os.path.join(WORK, "lib")       # config 里的 directory（move 不该用它）
MOVEDIR = os.path.join(WORK, "moved")    # move_items 显式传的目标根
SRCDIR = os.path.join(WORK, "src")
for d in (BEETSDIR, LIBDIR, MOVEDIR, SRCDIR):
    os.makedirs(d)
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    f.write(f"directory: {LIBDIR}\nimport:\n    copy: no\n")

ENV = dict(os.environ, BEETSDIR=BEETSDIR, PYTHONIOENCODING="utf-8", PYTHONUTF8="1")


def run(cmds, timeout=180):
    """一次性 sidecar 进程按序跑多条命令，返回解析后的事件列表。"""
    proc = subprocess.Popen(
        [PYTHON, SIDECAR],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        text=True, encoding="utf-8", env=ENV,
    )
    payload = "\n".join(json.dumps(c, ensure_ascii=False) for c in cmds) + "\n"
    try:
        out, _ = proc.communicate(payload, timeout=timeout)
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
            print("skip non-JSON:", line[:120])
            continue
        events.append(obj)
    return events, proc.returncode


def of(events, typ):
    return [e for e in events if e.get("type") == typ]


def beets_items():
    sys.path.insert(0, os.path.join(REPO, "tools", "beets-venv", "Lib", "site-packages"))
    os.environ["BEETSDIR"] = BEETSDIR
    import beets
    from beets.library import Library
    lib = Library(
        beets.config["library"].as_filename(),
        beets.config["directory"].as_filename(),
    )
    return lib, list(lib.items())


# ---- 准备素材：3 个 wav，其中两个同标题（查重用）----
from mutagen.id3 import TIT2, TPE1
from mutagen.wave import WAVE

SRC = []
for i, p in enumerate(SRC_WAV):
    dst = os.path.join(SRCDIR, os.path.basename(p))
    shutil.copy2(p, dst)
    tags = WAVE(dst)
    # wav 的标签是 ID3（不是 vorbis comment），必须塞 Frame 实例；
    # 测试 wav 本来没有 ID3 chunk，先 add_tags() 建一个
    tags.add_tags()
    tags.tags.add(TIT2(encoding=3, text="曲目A" if i < 2 else "曲目B"))
    tags.tags.add(TPE1(encoding=3, text="冒烟艺人"))
    tags.save()
    SRC.append(dst)

# ---- 场景 1：adopt 把文件按现状登记进 beets 库（幂等）----
events, rc = run([{"id": 1, "cmd": "adopt_library", "paths": [SRCDIR]}])
done = of(events, "adoptDone")
assert done, events
assert done[0]["added"] == 3 and done[0]["failed"] == 0, done[0]
assert rc == 0, rc
_lib, items = beets_items()
assert len(items) == 3, len(items)
assert all(os.path.exists(it.path.decode("utf-8")) for it in items), "文件应留在原地"
print("[1] adopt OK: added=3, 文件未移动")

events, rc = run([{"id": 1, "cmd": "adopt_library", "paths": [SRCDIR]}])
done = of(events, "adoptDone")[0]
assert done["added"] == 0 and done["skipped"] == 3, done
print("[1b] adopt 幂等 OK: skipped=3")

# ---- 场景 2：update —— mtime 驱动、pretend 不写库 ----
# mtime 没变 → 全部 unchanged（证明预筛生效，不是"每次全量重读"）
events, rc = run([{"id": 2, "cmd": "update_library", "pretend": True}])
done = of(events, "updateDone")[0]
assert done["unchanged"] == 3 and done["changed"] == 0, done
print("[2] update 预筛 OK: mtime 未变 → unchanged=3")

# 改一个文件的标签（磁盘上动，库不动）
tags = WAVE(SRC[2])
tags.tags.delall("TIT2")
tags.tags.add(TIT2(encoding=3, text="曲目B改"))
tags.save()
# 显式推 mtime 到未来，避免文件系统时间戳精度把"刚刚改过"判成本次不变
future = os.path.getmtime(SRC[2]) + 5
os.utime(SRC[2], (future, future))

events, rc = run([{"id": 3, "cmd": "update_library", "pretend": True}])
done = of(events, "updateDone")[0]
assert done["changed"] == 1 and done["pretend"] is True, done
assert done["details"][0]["changes"]["title"] == {"from": "曲目B", "to": "曲目B改"}, done["details"]
_lib, items = beets_items()
assert {it.title for it in items} == {"曲目A", "曲目B"}, "pretend 不能改库"
print("[2b] update 预览 OK: changed=1, 库未变")

events, rc = run([{"id": 4, "cmd": "update_library", "pretend": False}])
done = of(events, "updateDone")[0]
assert done["changed"] == 1, done
_lib, items = beets_items()
assert {it.title for it in items} == {"曲目A", "曲目B改"}, {it.title for it in items}
print("[2c] update 应用 OK: 库内 title 已更新")

# ---- 场景 3：find_duplicates —— 只读，按 title 分组 ----
events, rc = run([{"id": 5, "cmd": "find_duplicates", "keys": ["title"]}])
done = of(events, "duplicatesDone")[0]
groups = of(events, "duplicateGroup")
assert done["groups"] == 1, done
assert groups[0]["key"] == ["曲目A"], groups[0]
assert groups[0]["count"] == 2 and len(groups[0]["items"]) == 2, groups[0]
assert all(g["size"] > 0 for g in groups[0]["items"]), "size 应来自磁盘"
print("[3] 查重 OK: 1 组 2 条（曲目A）")

# 全空 key 不参与分组：改成空标题后不应把三首都塞进一组
_lib, items = beets_items()
for it in items:
    it.title = ""
    it.store()
events, rc = run([{"id": 6, "cmd": "find_duplicates", "keys": ["title"]}])
done = of(events, "duplicatesDone")[0]
assert done["groups"] == 0, done
# 恢复标题，但把重复的那首改成不同标题：下面的 move 需要三个不同的目标路径
# （同名 → 目标撞一起 → 全是 conflict，那是场景 4d 单独验的语义）
for it, t in zip(sorted(items, key=lambda x: x.id), ["曲目A", "曲目C", "曲目B改"]):
    it.title = t
    it.store()
print("[3b] 空 key 跳过 OK: groups=0")

# ---- 场景 4：move —— 库根只认 config，宿主传的只作校验 ----
# 4a 不一致的库根必须拒绝（宁可报错，也不要在错位置搬 1800 个文件）
events, rc = run([{"id": 7, "cmd": "move_items", "directory": MOVEDIR, "pretend": True}])
failed = [e for e in events if e.get("type") == "sessionFailed"]
assert failed, events
assert "不一致" in failed[0]["error"]["beets_message"], failed[0]
print("[4a] 库根不一致拒绝 OK:", failed[0]["error"]["beets_message"][:50])

# 4b config 里没有 directory → 拒绝（绝不能回落到 ~/Music）
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    f.write("import://n    copy: no\n")
events, rc = run([{"id": 8, "cmd": "move_items", "directory": "", "pretend": True}])
failed = [e for e in events if e.get("type") == "sessionFailed"]
assert failed, events
assert "没有 directory" in failed[0]["error"]["beets_message"], failed[0]
print("[4b] 未配置库根拒绝 OK:", failed[0]["error"]["beets_message"][:40])
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    f.write(f"directory: {LIBDIR}\nimport://n    copy: no\n")

# 4c 预览：不动文件、不建目录
events, rc = run([{"id": 9, "cmd": "move_items", "directory": LIBDIR, "pretend": True}])
done = of(events, "moveDone")[0]
plan = of(events, "moveItem")
assert done["pretend"] is True and done["planned"] == 3, done
assert all(p["dest"].lower().startswith(LIBDIR.lower()) for p in plan), plan
assert all(os.path.exists(p["path"]) for p in plan), "预览不能动文件"
assert not os.listdir(LIBDIR), "预览不能建文件"
print("[4c] move 预览 OK: planned=3")

# 4d 执行：文件真的搬走，源目录清空
events, rc = run([{"id": 10, "cmd": "move_items", "directory": LIBDIR, "pretend": False}])
done = of(events, "moveDone")[0]
assert done["moved"] == 3 and done["conflict"] == 0 and done["failed"] == 0, done
_lib, items = beets_items()
new_paths = [it.path.decode("utf-8") for it in items]
assert all(p.lower().startswith(LIBDIR.lower()) for p in new_paths), new_paths
assert all(os.path.exists(p) for p in new_paths), "库内路径应真实存在"
assert not os.listdir(SRCDIR), "源目录应被搬空（beets move 清理空目录）"
print("[4d] move 执行 OK:", os.path.basename(new_paths[0]))

# 4e 幂等：目标 == 源 → 全部 same
events, rc = run([{"id": 11, "cmd": "move_items", "directory": LIBDIR, "pretend": False}])
done = of(events, "moveDone")[0]
assert done["same"] == 3 and done["planned"] == 0 and done["moved"] == 0, done
print("[4e] move 幂等 OK: same=3")

# 4f 库根写成 ~/xxx → 必须展开后再判存在（不是拼出 <cwd>/~ 去搬文件）
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    f.write("directory: ~/aiya-maint-nonexistent\n")
events, rc = run([{"id": 13, "cmd": "move_items", "directory": "", "pretend": True}])
failed = [e for e in events if e.get("type") == "sessionFailed"]
assert failed, events
msg = failed[0]["error"]["beets_message"]
assert "~" not in msg, f"路径没展开就去搬文件：{msg}"
assert os.path.expanduser("~").lower() in msg.lower(), msg
print("[4f] ~ 展开 OK:", msg[:60])
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    f.write(f"directory: {LIBDIR}\nimport://n    copy: no\n")

# ---- 场景 5：目标冲突 —— 不改名、不覆盖（beets 自己会静默改成 name.1.ext）----
_lib, items = beets_items()
victim = sorted(items, key=lambda x: x.id)[2]
victim.title = "曲目C"  # 与 id2 的目标路径同名
victim.store()
existing = os.path.join(LIBDIR, "Non-Album", "冒烟艺人", "曲目C.wav")
assert os.path.exists(existing), existing
sentinel_size = os.path.getsize(existing)
events, rc = run([{"id": 12, "cmd": "move_items", "directory": LIBDIR, "pretend": False}])
done = of(events, "moveDone")[0]
assert done["conflict"] == 1 and done["moved"] == 0, done
assert os.path.getsize(existing) == sentinel_size, "目标文件被覆盖了"
assert os.path.exists(victim.path.decode("utf-8")), "冲突时源文件不该被动"
print("[5] 冲突不覆盖 OK:", done["errors"][0]["message"][:40])

# ---- 场景 6：确认作用域 —— 只动 beets 库，没碰别的东西 ----
_lib, items = beets_items()
assert len(items) == 3, len(items)
print("[6] 库内仍是 3 条，未被维护操作污染")

shutil.rmtree(WORK, ignore_errors=True)
print("ALL PASS")
