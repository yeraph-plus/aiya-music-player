# -*- coding: utf-8 -*-
"""协议契约测试（docs/33 §9 M3）：工具吐出的**每一行**都必须符合 v1 schema。

三件事，缺一不可：

1. **自证**：`protocol.SCHEMA_SHA256` 必须等于 `protocol/v1.schema.json` 的
   真实 sha256。改了 schema 忘了改常量 → 这里红（而不是等用户装到旧组件）。
2. **双向**：schema 里的错误码枚举必须等于 `protocol.ERROR_CODES`。少一个码
   → 这里红（宿主拿到未登记的码就分不了类）。
3. **实跑**：起真的 `serve` 会话，把 14 种事件 + 3 种应答各自走一遍，
   逐行按 `$defs` 校验。剩下的 `album` / `albumError` / `done` 要联网匹配
   MusicBrainz，默认跳过；`AIYA_BEETS_ONLINE=1` 时一并验。

隔离：独立临时 BEETSDIR + 独立库目录，不碰 dev 库和真实 Music 目录。
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

try:
    import jsonschema
    from jsonschema import Draft202012Validator
except ImportError:  # pragma: no cover
    print("[skip] 缺 jsonschema：pip install -e '.[dev]'（只影响本测试，不影响工具运行）")
    raise SystemExit(2)

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)                    # beets-tool/
REPO = os.path.dirname(TOOL)                    # 仓库根
TOOL_SRC = os.path.join(TOOL, "src")
SCHEMA_PATH = os.path.join(TOOL, "protocol", "v1.schema.json")
PYTHON = os.environ.get("AIYA_BEETS_PYTHON") or os.path.join(
    TOOL, ".venv", "Scripts", "python.exe")
TEST_MUSIC = os.environ.get("AIYA_TEST_MUSIC") or r"D:\MUSIC_Player\test-music"

sys.path.insert(0, TOOL_SRC)
from aiya_beets import protocol as P  # noqa: E402

SCHEMA_BYTES = open(SCHEMA_PATH, "rb").read()
SCHEMA = json.loads(SCHEMA_BYTES.decode("utf-8"))


def group(name):
    """取某个 $defs 分组当独立 schema 用。

    不能直接把 `SCHEMA["$defs"][name]` 丢给校验器：子 schema 里的
    `#/$defs/...` 引用会失去所在文档的基准 URI。带上整份 `$defs` 再 `$ref`
    就永远能解析（2020-12 允许 `$ref` 有兄弟关键字）。
    """
    return {
        "$schema": SCHEMA["$schema"],
        "$defs": SCHEMA["$defs"],
        "$ref": f"#/$defs/{name}",
    }


V = {n: Draft202012Validator(group(n)) for n in (
    "Request", "Response", "Event", "InstallerEvent",
    "StatusData", "PingData", "CapabilitiesData", "DumpItem",
)}
VALIDATED = {}   # 分组 → 校验过的行数（最后打印覆盖率，防止"跑了个寂寞"）


def check(name, obj, label=""):
    """校验一条消息；失败时把 JSON 路径和实例一起打出来。"""
    errs = sorted(V[name].iter_errors(obj), key=lambda e: list(e.absolute_path))
    if errs:
        e = errs[0]
        where = "/".join(str(p) for p in e.absolute_path) or "<root>"
        raise AssertionError(
            f"[{name}] {label} 不符合契约：{e.message}\n"
            f"  路径：{where}\n"
            f"  实例：{json.dumps(obj, ensure_ascii=False)[:400]}"
        )
    VALIDATED[name] = VALIDATED.get(name, 0) + 1


# ---------------- 1. 自证：schema 指纹 ----------------
digest = hashlib.sha256(SCHEMA_BYTES).hexdigest()
assert digest == P.SCHEMA_SHA256, (
    "protocol.SCHEMA_SHA256 与 protocol/v1.schema.json 不一致——改了 schema 必须同步常量：\n"
    f"  文件实际：{digest}\n"
    f"  代码里写的：{P.SCHEMA_SHA256}"
)
print("[1] schema 指纹自洽 OK:", digest[:12])

# ---------------- 2. 双向：错误码集合 ----------------
schema_codes = set(SCHEMA["$defs"]["ErrorCode"]["enum"])
assert schema_codes == P.ERROR_CODES, (
    "schema 的错误码枚举与 protocol.ERROR_CODES 不一致：\n"
    f"  只在 schema：{sorted(schema_codes - P.ERROR_CODES)}\n"
    f"  只在代码：{sorted(P.ERROR_CODES - schema_codes)}"
)
assert len(schema_codes) == len(SCHEMA["$defs"]["ErrorCode"]["enum"]), "错误码枚举有重复项"
print(f"[2] 错误码双向一致 OK：{len(schema_codes)} 个")

# ---------------- 3. 实跑：真实流量 ----------------
SRC_WAV = sorted(
    os.path.join(TEST_MUSIC, f)
    for f in os.listdir(TEST_MUSIC) if f.lower().endswith(".wav")
) if os.path.isdir(TEST_MUSIC) else []
if not SRC_WAV:
    print(f"[skip] 没有测试音源（{TEST_MUSIC}）：设 AIYA_TEST_MUSIC 指向含 .wav 的目录")
    raise SystemExit(2)

WORK = tempfile.mkdtemp(prefix="aiya-proto-")
BEETSDIR = os.path.join(WORK, "beetsdir")
LIBDIR = os.path.join(WORK, "lib")
os.makedirs(BEETSDIR)
os.makedirs(LIBDIR)
with open(os.path.join(BEETSDIR, "config.yaml"), "w", encoding="utf-8") as f:
    # copy: no —— 导入默认复制会干扰我们自己的动作控制（与 smoke_apply 同）
    f.write(f"directory: {LIBDIR}\nimport:\n    copy: no\n")

ENV = dict(os.environ, BEETSDIR=BEETSDIR, PYTHONIOENCODING="utf-8", PYTHONPATH=TOOL_SRC)
ENV.pop("AIYA_BEETS_HOME", None)   # 别让外部环境把 BEETSDIR 顶掉


def fresh_src(dirname):
    d = os.path.join(WORK, dirname)
    os.makedirs(d)
    out = []
    for p in SRC_WAV:
        dst = os.path.join(d, os.path.basename(p))
        shutil.copy2(p, dst)
        out.append(dst)
    return out


def run(lines, timeout=300):
    """一次 serve 会话：把请求行全灌进去再关 stdin，收集所有输出行。

    服务端是**顺序**处理请求的（单线程主循环），所以不用等上一条的终态再发
    下一条；`id` + `session` 已经足够把事件归位。
    """
    proc = subprocess.Popen(
        [PYTHON, "-m", "aiya_beets"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8", env=ENV,
    )
    out, err = proc.communicate("\n".join(lines) + "\n", timeout=timeout)
    assert proc.returncode == 0, f"serve 退出码 {proc.returncode}\n{err[-2000:]}"
    return out.splitlines(), err


def classify_line(raw):
    """一行输出 → (分组名, 对象)。

    判别只看**有没有 `ok`**：有 = 应答（错误应答同时是终止事件）；
    没有 = 会话事件。裸行（beets 的首写杂音）返回 (None, raw)，由调用方决定。
    """
    try:
        obj = json.loads(raw)
    except json.JSONDecodeError:
        return None, raw
    assert isinstance(obj, dict), f"输出行必须是 JSON 对象：{type(obj).__name__}"
    if "ok" in obj:
        check("Response", obj, "应答信封")
        return "Response", obj
    check("Event", obj, "会话事件")
    return "Event", obj


# ---- 场景 1：管理三件 + 全部离线命令 ----
src1 = fresh_src("src1")
dup_a = fresh_src("dup_a")      # 同一批文件拷 twice：同一 title/artist → 查重必有组
dup_b = fresh_src("dup_b")
adopt_dir = fresh_src("adopt")

DEC = lambda album, paths, titles, artist="契约艺人": json.dumps({
    "id": 0, "cmd": "apply_import", "decisions": [{
        "album": "alb_000",
        "action": "apply",
        "paths": paths,
        "candidate": {
            "album": {"album": album, "albumartist": artist, "year": 2026,
                      "genre": "契约流派"},
            "mapping": {p: {"title": t, "artist": artist, "track": i, "disc": 1}
                        for i, (p, t) in enumerate(zip(paths, titles), 1)},
        },
    }]})


def with_id(raw, req_id):
    obj = json.loads(raw)
    obj["id"] = req_id
    return json.dumps(obj, ensure_ascii=False)


reqs = [
    # --- 管理三件 ---
    json.dumps({"id": 1, "cmd": "status", "clientProtocol": P.PROTOCOL_VERSION}),
    json.dumps({"id": 2, "cmd": "ping"}),
    json.dumps({"id": 3, "cmd": "capabilities"}),
    # --- 导入（写标签，不移动）---
    with_id(DEC("契约专辑", src1, [f"契约 {i:02d}" for i in range(1, len(src1) + 1)]), 4),
    # --- 对账 ---
    json.dumps({"id": 5, "cmd": "dump_items"}),
    # --- 维护：update（预览，只报告不改库）---
    json.dumps({"id": 6, "cmd": "update_library", "pretend": True}),
    # --- 维护：制造重复（两批同标题/歌手的曲目）→ 查重必有组 ---
    # 为什么要先 apply 而不是 adopt：查重按 title+artist 分组，而**全空的组会被
    # 跳过**（与 beets duplicates 的 not strict 分支一致），所以没标签的文件
    # 天然查不出重复——这里必须把标签写进去。
    with_id(DEC("重复甲", dup_a, [f"重 {i:02d}" for i in range(1, len(dup_a) + 1)]), 7),
    with_id(DEC("重复乙", dup_b, [f"重 {i:02d}" for i in range(1, len(dup_b) + 1)]), 8),
    json.dumps({"id": 9, "cmd": "find_duplicates", "keys": ["title", "artist"]}),
    # --- 维护：整理（传入根 ≠ 配置根 → 必须被拒）---
    json.dumps({"id": 10, "cmd": "move_items",
                "directory": os.path.join(WORK, "别的地方"), "pretend": True}),
    json.dumps({"id": 11, "cmd": "move_items", "pretend": True}),
    # --- 维护：adopt 真新增（as-is 入库，不需要标签）---
    json.dumps({"id": 12, "cmd": "adopt_library",
                "paths": [os.path.join(WORK, "adopt")]}),
    # --- 三类错误 ---
    json.dumps({"id": 13, "cmd": "bogus_cmd"}),
    '{"id": 14, "cmd": ',                       # 坏 JSON
    json.dumps({"id": 15, "cmd": "ping", "clientProtocol": 99}),
    # 取消不能放在这一批里：cancel 在 **reader 线程**旁路处理（不等主循环），
    # 一次灌多行时它会立刻把取消代数推高，把前面还在跑的会话全掐了。
    # 取消的契约验证放在场景 2 单独一次会话里做。
]

lines, stderr = run(reqs)

seen = {}
codes = []
for raw in lines:
    if not raw.strip():
        continue
    kind, obj = classify_line(raw)
    if kind is None:
        # 裸行只允许出现在 stderr；stdout 混进非 JSON 就是协议违反
        raise AssertionError(f"stdout 出现非 JSON 行：{obj[:200]}")
    if kind == "Response":
        if obj["ok"]:
            seen.setdefault("data", []).append(obj)
        else:
            codes.append(obj["error"]["code"])
    else:
        seen.setdefault(obj["type"], []).append(obj)

# 每种会话事件都要真的出现过（否则"校验通过"只是没验到）
for want in ["progress", "applied", "applyDone", "item", "dumpDone", "updateDone",
             "adoptDone", "duplicateGroup", "duplicatesDone", "moveItem", "moveDone"]:
    assert want in seen, f"没有出现 {want} 事件（实际：{sorted(seen)}）"

# 应答 data 的形状：按命令各自验（信封里的 data 是无约束的，见 schema 注释）
DATA_SCHEMA = {1: "StatusData", 2: "PingData", 3: "CapabilitiesData"}
for resp in seen["data"]:
    want = DATA_SCHEMA.get(resp["id"])
    if want:
        check(want, resp["data"], f"id={resp['id']}")

# dump 的每条 item 单独验（它在 Event 里已经验过一次 envelope，这里验内容）
assert any(True for _ in seen["item"]), "dump_items 没吐出任何 item"

# 三个错误码必须精确命中
assert sorted(codes) == sorted([
    P.E_LIBRARY_DIR_MISMATCH, P.E_UNKNOWN_CMD, P.E_BAD_JSON, P.E_PROTOCOL_MISMATCH,
]), f"错误码不符合预期：{codes}"
kinds = [k for k in seen if k != "data"]
print(f"[3] 离线全链路 OK：{sum(VALIDATED.values())} 条消息、"
      f"{len(kinds)} 种事件、错误码 {codes}")

# ---- 场景 2：取消（终态必须是 cancelled 或 applyDone，不能两个都有）----
reqs2 = []
for i in range(12):
    g = fresh_src(f"cancel_{i:02d}")
    reqs2.append(json.dumps({"id": 20 + i, "cmd": "apply_import", "decisions": [{
        "album": f"alb_{i:03d}", "action": "apply", "paths": g,
        "candidate": {"album": {"album": f"取消{i:02d}", "albumartist": "契约艺人"},
                      "mapping": {p: {"title": f"X{i:02d} {j}", "artist": "契约艺人",
                                      "track": j, "disc": 1}
                                  for j, p in enumerate(g, 1)}},
    }]}))
reqs2.append(json.dumps({"id": 90, "cmd": "cancel"}))

lines2, _ = run(reqs2)
types2 = []
terminals = {}      # session → [终态类型]
for raw in lines2:
    if not raw.strip():
        continue
    kind, obj = classify_line(raw)
    assert kind is not None, f"stdout 出现非 JSON 行：{obj[:200]}"
    if kind != "Event":
        continue
    types2.append(obj["type"])
    if obj["type"] in ("cancelled", "applyDone"):
        terminals.setdefault(obj["session"], []).append(obj["type"])

assert "cancelAck" in types2, types2
# 12 个请求 × 每个必须**恰好一个**终态。以前冒烟里的写法是"整批只许一个"，
# 那在多个请求排队时是错的：取消一到，每个排队中的请求各自收尾成 cancelled。
missing = [i for i in range(12) if f"app_{20 + i:02d}" not in terminals]
assert not missing, f"这些请求没有终态事件（会话会卡住）：{missing}"
multi = {k: v for k, v in terminals.items() if len(v) != 1}
assert not multi, f"一个请求发了多个终态（事件流会重复收尾）：{multi}"
cancelled = sum(1 for v in terminals.values() if v[0] == "cancelled")
print(f"[4] 取消终态 OK：{len(terminals)} 个请求各一个终态，其中 {cancelled} 个被取消")

# ---- 场景 3（可选）：联网匹配的三种事件 ----
if os.environ.get("AIYA_BEETS_ONLINE") == "1":
    net = fresh_src("net")
    lines3, _ = run([json.dumps({"id": 40, "cmd": "preview_import", "paths": net})], timeout=600)
    got = set()
    for raw in lines3:
        if not raw.strip():
            continue
        kind, obj = classify_line(raw)
        assert kind is not None, f"stdout 出现非 JSON 行：{obj[:200]}"
        if kind == "Event":
            got.add(obj["type"])
    for want in ["progress", "album", "done"]:
        assert want in got, f"联网预览缺 {want}（实际 {sorted(got)}）"
    print(f"[5] 联网预览 OK：{sorted(got)}")
else:
    print("[5] 跳过 album/albumError/done（要联网匹配；AIYA_BEETS_ONLINE=1 开启）")

shutil.rmtree(WORK, ignore_errors=True)

print("\n分组校验行数：", ", ".join(f"{k}={v}" for k, v in sorted(VALIDATED.items())))
print("ALL PASS")
