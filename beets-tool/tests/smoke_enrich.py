# -*- coding: utf-8 -*-
"""索引增强 / 查询的冒烟（docs/45 §4.2）：真 beets + 真进程，走一遍全链路。

验的不是"函数返回对不对"，而是四件只有跑真进程才暴露的事：

1. **协议形状**：`enrichItem` / `enrichDone` / `query` / `fields` 的每一行都过
   schema。手写 JSON 的单测发现不了 DTO 漂移——只有对着对面发的 schema 验才算。
2. **文件不动**：整场跑完音频文件字节不变。"索引家永不写文件"是拍板前提，
   靠代码自觉守不住，得有断言。
3. **子进程不偷 stdin**：`enrich` 内部起 `beet import` 子进程，如果没把它的
   stdin 掐掉，它会跟主循环抢管道里的请求行——症状是"后面的请求凭空消失"。
4. **幂等**：同一批路径跑两遍（第二遍 `force`），字段值一字不差。

隔离：独立临时 BEETSDIR + 独立库目录。假源靠 `AIYA_BEETS_ENRICH_PROBE=1`
开启，默认关闭（假数据进真库是最难排查的污染）。
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

try:
    import jsonschema  # noqa: F401
    from jsonschema import Draft202012Validator
except ImportError:  # pragma: no cover
    print("[skip] 缺 jsonschema：pip install -e '.[dev]'（只影响本测试，不影响工具运行）")
    raise SystemExit(2)

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
TOOL_SRC = os.path.join(TOOL, "src")
SCHEMA_PATH = os.path.join(TOOL, "protocol", "v2.schema.json")
PYTHON = os.environ.get("AIYA_BEETS_PYTHON") or os.path.join(
    TOOL, ".venv", "Scripts", "python.exe")
sys.path.insert(0, TOOL_SRC)
sys.path.insert(0, HERE)
from _testmusic import resolve as _resolve_test_music  # noqa: E402
from aiya_beets import protocol as P  # noqa: E402

SCHEMA = json.loads(open(SCHEMA_PATH, "rb").read().decode("utf-8"))


def group(name):
    return {"$schema": SCHEMA["$schema"], "$defs": SCHEMA["$defs"], "$ref": f"#/$defs/{name}"}


V = {n: Draft202012Validator(group(n)) for n in ("Request", "Response", "Event")}
VALIDATED = {}


def check(name, obj, label):
    errs = sorted(V[name].iter_errors(obj), key=lambda e: list(e.absolute_path))
    if errs:
        e = errs[0]
        where = "/".join(str(p) for p in e.absolute_path) or "<root>"
        raise AssertionError(
            f"[{name}] {label} 不符合契约：{e.message}\n"
            f"  路径：{where}\n  实例：{json.dumps(obj, ensure_ascii=False)[:400]}"
        )
    VALIDATED[name] = VALIDATED.get(name, 0) + 1


# ---------------- 隔离环境 ----------------
# 刻意走**真的 index 家**（AIYA_BEETS_PROFILE=index）而不是手写一份 config.yaml：
# 索引家的模板里钉着 `write/copy/move: no` 和 `duplicate_action: keep`，后者
# 正是"同标签同长的第二个文件能不能进库"的开关——手写配置很容易漏，而漏了
# 之后症状是"只导进去 1/6 个文件"，看起来像路径 bug。用真模板等于顺带验了
# "批 1 的模板足够支撑批 2 的流程"。
SRC_WAV = _resolve_test_music(3)
WORK = tempfile.mkdtemp(prefix="aiya-enrich-")
HOME = os.path.join(WORK, "home")
LIBDIR = HOME                       # 索引家的 library.db 就在 home 下
MUSIC = os.path.join(WORK, "music")
os.makedirs(HOME)

ENV = dict(
    os.environ,
    AIYA_BEETS_HOME=HOME,
    AIYA_BEETS_PROFILE="index",
    PYTHONIOENCODING="utf-8",
    PYTHONPATH=TOOL_SRC,
    AIYA_BEETS_ENRICH_PROBE="1",
)
# 外部 BEETSDIR 会把家顶掉（cfg.apply_env 遇两者不等直接拒绝，但那属于
# 另一种失败）。这里清掉，让工具自己下发。
ENV.pop("BEETSDIR", None)


def make_files(n_dirs=2, per_dir=3, root=None):
    """造测试文件。`root` 可换：同一 root 下重复调用会**覆盖**同名文件，
    而指纹断言依赖"跑完之后文件字节不变"，被覆盖就测不出东西了。"""
    base = root or MUSIC
    out = []
    for d in range(n_dirs):
        sub = os.path.join(base, f"alb{d:02d}")
        os.makedirs(sub)
        for i in range(per_dir):
            src = SRC_WAV[i % len(SRC_WAV)]
            dst = os.path.join(sub, f"{d:02d}-{i:02d}-{os.path.basename(src)}")
            shutil.copy2(src, dst)
            out.append(dst)
    return out


def fingerprint(paths):
    """文件字节指纹：用来证明整场跑完文件没被写过。"""
    h = {}
    for p in paths:
        with open(p, "rb") as f:
            h[p.lower()] = hashlib.sha256(f.read()).hexdigest()
    return h


def run(lines, env=None, timeout=600):
    proc = subprocess.Popen(
        [PYTHON, "-m", "aiya_beets"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8", env=env or ENV,
    )
    out, err = proc.communicate("\n".join(lines) + "\n", timeout=timeout)
    assert proc.returncode == 0, f"serve 退出码 {proc.returncode}\n{err[-2000:]}"
    return out.splitlines(), err


def run_cold(lines, timeout=600):
    """在**全新的家**里跑，第一条命令就是它。

    验的是"空库首跑"这条真实路径：用户刚打开索引功能、库还不存在时，
    三段式仍然要走通（补登会创建库、清旧无对象可清、分析照常）。
    前面的场景是"库已经存在"，两条路径的成功条件不同。
    """
    home = os.path.join(WORK, f"cold{len(VALIDATED)}{os.getpid()}")
    os.makedirs(home, exist_ok=True)
    env = dict(ENV, AIYA_BEETS_HOME=home)
    return run(lines, env=env, timeout=timeout)


def classify(raw):
    obj = json.loads(raw)          # 非 JSON 直接抛：stdout 只许 JSON 行
    assert isinstance(obj, dict), f"输出行必须是 JSON 对象：{type(obj).__name__}"
    if "ok" in obj:
        check("Response", obj, "应答")
        return "Response", obj
    check("Event", obj, "事件")
    return "Event", obj


PATHS = make_files()
BEFORE = fingerprint(PATHS)
print(f"[0] 环境就绪：{len(PATHS)} 个文件，库 {LIBDIR}")

# ---------------- 场景 1：fields ----------------
L1 = [json.dumps({"id": 1, "cmd": "fields"})]
lines, _ = run(L1)
resps = {}
for raw in lines:
    kind, obj = classify(raw)
    if kind == "Response":
        resps[obj["id"]] = obj

fields_data = resps[1]["data"]
names = {f["name"] for f in fields_data["fields"]}
assert "title" in names and "bpm" in names, f"内置字段缺失，只有 {sorted(names)[:10]}"
assert all(f["source"] in ("builtin", "flexible") for f in fields_data["fields"])
src_states = {s["name"]: s["state"] for s in fields_data["sources"]}
assert src_states["probe"] == "ready", src_states
assert src_states["xtractor"] == "unwired", src_states
print(f"[1] fields OK：{len(names)} 个字段，源 {src_states}")

# ---------------- 场景 2：enrich（首跑） ----------------
L2 = [json.dumps({"id": 2, "cmd": "enrich", "paths": PATHS, "want": ["probe"]})]
lines, _ = run(L2)
items, done = [], None
for raw in lines:
    kind, obj = classify(raw)
    if kind != "Event":
        continue
    if obj["type"] == "enrichItem":
        items.append(obj)
    elif obj["type"] == "enrichDone":
        done = obj

assert done is not None, "缺 enrichDone（会话会卡在终态上）"
assert done["session"] == "enr_02", done["session"]
assert done["total"] == len(PATHS) == done["succeeded"], done
assert done["failed"] == 0 and done["skipped"] == 0, done
assert done["sources"] == ["probe"], done["sources"]
assert len(items) == len(PATHS), f"enrichItem 条数 {len(items)} ≠ 路径数 {len(PATHS)}"
first = {i["path"]: i for i in items}
assert all(i["status"] == "ok" for i in items)
assert all(i["fields"] for i in items), "ok 却没有任何字段"
bpm_by_path = {p: i["fields"]["bpm"] for p, i in first.items()}
print(f"[2] enrich OK：{done['total']} 条 ok，bpm 样本 {list(bpm_by_path.values())[:3]}")

# 7 个 mood_* 概率：宿主拿它们 argmax 出单列 mood_label（docs/45 §5.1）。
# 假源少喂这一类，宿主那半边（提升列/按情绪筛）就永远测不到。
MOODS = ("mood_happy", "mood_sad", "mood_relaxed", "mood_aggressive",
         "mood_party", "mood_acoustic", "mood_electronic")
for p, it in first.items():
    missing = [m for m in MOODS if m not in it["fields"]]
    assert not missing, f"{p} 缺情绪概率 {missing}（宿主 argmax 会拿不到结论）"
    top = max(it["fields"][m] for m in MOODS)
    assert 0.0 <= top <= 1.0, f"{p} 情绪概率越界：{top}"
    assert top > 1.0 / len(MOODS), f"{p} 的分布太平，argmax 无意义：{top}"
print(f"[2b] 情绪概率 OK：{len(MOODS)} 类齐备，argmax 必胜出")

# ---------------- 场景 3：query（结构化规则 / raw / 边界） ----------------
lo = min(bpm_by_path.values())
L3 = [
    json.dumps({"id": 3, "cmd": "query", "fields": ["title", "bpm"],
                "rules": [{"field": "bpm", "op": "gte", "value": lo}]}),
    # 数值范围：这是"结构化规则在工具侧求值"的核心价值——不依赖 item_types 注册
    json.dumps({"id": 4, "cmd": "query", "fields": ["bpm"],
                "rules": [{"field": "bpm", "op": "range", "value": [0, 400]}]}),
    # 同样的范围换成 beets 原生语法：`danceability` 是**未注册类型的 flexible
    # 字段**，beets 把它当字符串，`0..1` 静默 0 命中。这条断言把"为什么数值
    # 筛选用结构化规则"钉在测试里（docs/45 §3.3）。
    # 反例证据：同样写法换成 `bpm`（内置 Integer 列）就会全命中——差别不在
    # beets 的查询能力，只在"这个字段有没有类型"。
    json.dumps({"id": 5, "cmd": "query", "raw": "danceability:0..1"}),
    # rules 与 raw 同时给 → 拒绝
    json.dumps({"id": 6, "cmd": "query", "raw": "bpm:1",
                "rules": [{"field": "bpm", "op": "eq", "value": 1}]}),
    # 未知算子 → 拒绝（库为空时也要报，所以在校验阶段拦）
    json.dumps({"id": 7, "cmd": "query",
                "rules": [{"field": "bpm", "op": "approximately", "value": 1}]}),
    json.dumps({"id": 8, "cmd": "query", "fields": ["title"], "limit": 2}),
]
lines, _ = run(L3)
resps, codes = {}, {}
for raw in lines:
    kind, obj = classify(raw)
    if kind == "Response":
        if obj["ok"]:
            resps[obj["id"]] = obj["data"]
        else:
            codes[obj["id"]] = obj["error"]["code"]

assert resps[3]["total"] == len(PATHS), resps[3]
assert resps[3]["truncated"] is False, resps[3]
assert all("title" in it["fields"] and "bpm" in it["fields"] for it in resps[3]["items"])
assert resps[4]["total"] == len(PATHS), f"数值 range 应全命中：{resps[4]['total']}"
assert resps[5]["total"] == 0, (
    "原生语法对未注册类型的 flexible 字段本应 0 命中；若这里变了，"
    "说明 beets 的类型注册行为改了，docs/45 §3.3 的结论要重写"
)
assert codes.get(6) == P.E_INVALID_ARG, codes
assert codes.get(7) == P.E_INVALID_ARG, codes
assert resps[8]["total"] == len(PATHS) and resps[8]["truncated"] is True, resps[8]
assert len(resps[8]["items"]) == 2, "limit 没生效"
print(f"[3] query OK：结构化 {resps[3]['total']} / raw 命中 {resps[5]['total']} / 错误码 {codes}")

# ---------------- 场景 4：force 重跑（幂等）+ 不存在的路径 ----------------
# "清旧真的执行了"怎么证明？实测出的事实：
#   · 不删行直接重导 → beets 保留原有 flexible 属性（只换 rowid），所以
#     "id 变了"证明不了任何事；
#   · 真删行再重导 → 属性随行消失。
# 所以只有"打一个标记、看它消没消"能分辨。测试端直接写库（这是测试，
# 不是产品路径），断言仍走协议。
MARK = "aiya_probe_mark"
DB = os.path.join(HOME, "library.db")


def ids_of(items):
    want = {p.lower() for p in PATHS}
    return {it["path"]: it["id"] for it in items if it["path"].lower() in want}


def mark_items(ids, value="1"):
    import sqlite3
    conn = sqlite3.connect(DB)
    try:
        for iid in ids:
            conn.execute(
                "INSERT OR REPLACE INTO item_attributes(entity_id, key, value) VALUES(?,?,?)",
                (iid, MARK, value),
            )
        conn.commit()
    finally:
        conn.close()


def read_marks():
    lines, _ = run([json.dumps({"id": 21, "cmd": "query", "limit": 100,
                                "fields": ["title", MARK]})])
    out = {}
    total = None
    for raw in lines:
        kind, obj = classify(raw)
        if kind == "Response" and obj.get("ok"):
            total = obj["data"]["total"]
            for it in obj["data"]["items"]:
                out[it["path"]] = it["fields"].get(MARK)
    return out, total


lines, _ = run([json.dumps({"id": 19, "cmd": "query", "limit": 100})])
ids_before = {}
for raw in lines:
    kind, obj = classify(raw)
    if kind == "Response" and obj.get("ok"):
        ids_before = ids_of(obj["data"]["items"])
assert len(ids_before) == len(PATHS), ids_before
mark_items(ids_before.values())
marks_before, _ = read_marks()
assert all(v == "1" for v in marks_before.values()), marks_before

GHOST = os.path.join(MUSIC, "alb00", "根本没有这个文件.wav")
L4 = [
    json.dumps({"id": 9, "cmd": "enrich", "paths": PATHS + [GHOST],
                "want": ["probe"], "force": True}),
]
lines, _ = run(L4)
items2, done2 = [], None
for raw in lines:
    kind, obj = classify(raw)
    if kind != "Event":
        continue
    if obj["type"] == "enrichItem":
        items2.append(obj)
    elif obj["type"] == "enrichDone":
        done2 = obj

assert done2["succeeded"] == len(PATHS), done2
assert done2["skipped"] == 1, f"不存在的路径应记 skipped：{done2}"
ghost_hit = [i for i in items2 if i["path"] == GHOST]
assert len(ghost_hit) == 1 and ghost_hit[0]["status"] == "skipped", ghost_hit
after = {i["path"]: i["fields"]["bpm"] for i in items2 if i["status"] == "ok"}
assert after == bpm_by_path, "force 重跑后字段值变了：假源本应确定性，说明写读链路有漂移"

marks_after, total_after = read_marks()
assert set(marks_after) == set(marks_before), set(marks_before) ^ set(marks_after)
assert total_after == len(PATHS), f"库里有 {total_after} 行，期望 {len(PATHS)}（多了就是重复行）"
assert all(v is None for v in marks_after.values()), (
    f"force 之后标记还在（{marks_after}）：清旧没真的摘掉项，"
    "重导只是原地更新、旧字段全都留着"
)
print(f"[4] force 幂等 OK：{done2['succeeded']} 条 ok / {done2['skipped']} 条 skipped")

# ---------------- 场景 5：未知源 / 空路径 ----------------
L5 = [
    json.dumps({"id": 10, "cmd": "enrich", "paths": PATHS[:1], "want": ["no_such_source"]}),
    json.dumps({"id": 11, "cmd": "enrich", "paths": PATHS[:1], "want": ["xtractor"]}),
    json.dumps({"id": 12, "cmd": "enrich", "paths": []}),
]
lines, _ = run(L5)
codes = {}
for raw in lines:
    kind, obj = classify(raw)
    if kind == "Response" and not obj["ok"]:
        codes[obj["id"]] = obj["error"]["code"]
assert codes == {10: P.E_INVALID_ARG, 11: P.E_INVALID_ARG, 12: P.E_INVALID_ARG}, codes
print(f"[5] 拒绝路径 OK：{codes}")

# ---------------- 场景 6：文件一个字节都没动 ----------------
assert fingerprint(PATHS) == BEFORE, "音频文件被改写了——违反\"索引家永不写文件\"的前提"
print("[6] 文件未改动 OK")

# ---------------- 场景 7：冷启动（全新的家，第一条就是 enrich） ----------------
COLD = make_files(n_dirs=1, per_dir=2, root=os.path.join(WORK, "coldmusic"))
lines, err = run_cold([json.dumps({"id": 20, "cmd": "enrich", "paths": COLD, "want": ["probe"]})])
kinds = []
for raw in lines:
    kind, obj = classify(raw)      # 非 JSON 行在这里直接抛
    kinds.append(obj.get("type") or ("resp" if obj.get("ok") else "?"))
assert kinds.count("enrichItem") == len(COLD), kinds
assert kinds[-1] == "enrichDone", kinds
print(f"[7] 冷启动 OK：{len(lines)} 行全是合法协议帧（空库首跑也能走完三段式）")

total = sum(VALIDATED.values())
print(f"\n全部通过：{total} 条消息按 schema 校验（{VALIDATED}），"
      f"临时目录 {WORK}（可删）")
