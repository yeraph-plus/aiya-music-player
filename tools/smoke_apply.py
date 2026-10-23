# -*- coding: utf-8 -*-
"""B2 apply_import 冒烟：拷贝 WAV → 发 apply 决策 → 验证标签写入 + beets 入库。"""
import json
import os
import subprocess
import sys
import tempfile

REPO = r"D:\MUSIC_Player\AIYA_MUSIC_TAURI"
SIDECAR = os.path.join(REPO, "sidecar", "beets_sidecar.py")
PYTHON = os.path.join(REPO, "tools", "beets-venv", "Scripts", "python.exe")
BEETSDIR = os.path.join(REPO, "tools", "beets-home")
TEST_DIR = tempfile.gettempdir() + r"\aiya-apply-test"

files = sorted(
    os.path.join(TEST_DIR, f) for f in os.listdir(TEST_DIR) if f.lower().endswith(".wav")
)
print("files:", len(files))
assert len(files) == 3, files

mapping = {}
for i, p in enumerate(files, 1):
    mapping[p] = {
        "title": f"测试曲目 {i:02d}",
        "artist": "测试艺人",
        "track": i,
        "disc": 1,
    }

decision = {
    "album": "alb_test",
    "action": "apply",
    "paths": files,
    "candidate": {
        "album": {
            "album": "冒烟测试专辑",
            "albumartist": "测试艺人",
            "year": 2026,
            "genre": "测试流派",
        },
        "mapping": mapping,
    },
}

env = dict(os.environ, BEETSDIR=BEETSDIR, PYTHONIOENCODING="utf-8")
proc = subprocess.Popen(
    [PYTHON, SIDECAR],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    text=True, encoding="utf-8", env=env,
)
req = json.dumps({"id": 7, "cmd": "apply_import", "decisions": [decision]}, ensure_ascii=False)
out, _ = proc.communicate(req + "\n", timeout=120)

events = []
responses = []
for line in out.splitlines():
    if not line.strip():
        continue
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        print("NON-JSON LINE:", repr(line[:300]))
        raise
    (events if "type" in obj else responses).append(obj)

print("--- events ---")
for e in events:
    print(e["type"], json.dumps(e, ensure_ascii=False)[:160])

types = [e["type"] for e in events]
assert "progress" in types, types
assert types.count("applied") == 1, types
applied = [e for e in events if e["type"] == "applied"][0]
assert applied["written"] == 3, applied
assert applied["albumId"] == "alb_test", applied
assert "applyDone" in types, types
done = [e for e in events if e["type"] == "applyDone"][0]
assert done == {"type": "applyDone", "session": done["session"], "applied": 1, "skipped": 0, "failed": 0}, done

# 响应信封（stdin EOF 后 sidecar 正常退出，无 id 响应 —— apply 用事件流应答）
assert proc.returncode == 0, proc.returncode

# 1) 文件标签验证（mutagen 直读）
from mutagen.wave import WAVE
for i, p in enumerate(files, 1):
    tags = WAVE(p)
    assert str(tags["TIT2"]) == f"测试曲目 {i:02d}", (p, tags["TIT2"])
    assert str(tags["TALB"]) == "冒烟测试专辑", (p, tags["TALB"])
    assert str(tags["TPE1"]) == "测试艺人", (p, tags["TPE1"])
print("--- file tags OK ---")

# 2) beets 库验证
sys.path.insert(0, os.path.join(REPO, "tools", "beets-venv", "Lib", "site-packages"))
os.environ["BEETSDIR"] = BEETSDIR
import beets
from beets.library import Library
lib = Library(beets.config["library"].as_filename())
items = list(lib.items())
print("beets lib items:", len(items))
assert len(items) == 3, [str(it) for it in items]
by_title = {it.title: it for it in items}
it = by_title["测试曲目 01"]
assert it.album == "冒烟测试专辑" and it.albumartist == "测试艺人"
assert it.year == 2026 and it.genre == "测试流派" and it.track == 1
assert it.path.decode("utf-8", "replace").lower() == files[0].lower(), it.path
print("--- beets library OK ---")

# 3) 幂等验证：重放同一决策，不应产生重复行
proc = subprocess.Popen(
    [PYTHON, SIDECAR],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    text=True, encoding="utf-8", env=env,
)
out2, _ = proc.communicate(req + "\n", timeout=120)
events2 = [json.loads(l) for l in out2.splitlines() if l.strip() and "type" in json.loads(l)]
applied2 = [e for e in events2 if e["type"] == "applied"][0]
assert applied2["written"] == 3, applied2
lib2 = Library(beets.config["library"].as_filename())
assert len(list(lib2.items())) == 3, "重放后不应出现重复行"
print("--- idempotent replay OK ---")
print("ALL PASS")
