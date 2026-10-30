"""索引增强三段式：清旧 → 补登 → 分析（docs/45 §4.2/§4.3）。

## 为什么是"用户手动装 beets 之后才可能跑"的一段

beets 与 Essentia 都不进发布包（2026-10-30 拍板）：装不装是用户的选择，
所以这条链路**必须能整体不存在**——没装 beets 时 `serve` 早就回了
`E_NOT_INSTALLED`，宿主把它当"索引能力不可用"，其余功能零影响。

## 三段为什么是这个顺序

docs/45 §4.3 的原始顺序是 补登 → 清旧 → 分析，那会把文件从库里删掉再指望
它自己回来。正确的因果是：

1. **清旧**（仅 `force`）：把清单里的项从库里摘掉。字段随之消失，插件下次
   才会重新产出。
2. **补登**：把清单重新导进来。被摘掉的回到库里，本来就不在库的第一次进来。
   已在库且没被摘的，`-I` 让它们**重新走一遍 import 流程**（否则增量会直接
   跳过，插件根本不跑——实测 docs/45 §3.4）。
3. **分析**：跑源命令，逐个 (文件, 源) 回吐。

## 与宿主的分工（边界铁律，docs/44 §9.1）

本模块**只产出数据**，绝不往应用 DB 写行、也绝不写音频文件。`enrichItem`
的消费者是宿主进程，它持自己的扫描互斥去落库。理由三条：边界门禁、宿主
进程内的 scanning 锁 sidecar 物理上拿不到、sidecar 的写行逻辑对老版本应用
DB 会变成隐形迁移器。
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys

from ..beetslib import open_library
from ..protocol import E_INVALID_ARG, ProtocolError
from ..runtime import cancel_gen, emit

_STAGE = "enrich"

# 进度事件的节流粒度（见主循环里的说明）。
PROGRESS_EVERY = 5

# 补登的固定开关。逐个说明（都不是随手挑的）：
#   -A  noautotag   不做联网匹配：索引只要成员资格，匹配是 DB2 的事
#   -q  quiet       不问任何问题（没有 TTY，问了就是卡死）
#   -s  singletons  逐曲导入：同目录混多专辑时按专辑分会漏
#   -I  noincremental  不跳过"已导入的目录"：否则二次调用整批跳过
#   -R  incremental_skip_later（**不是** noresume）不把跳过项写进 state.pickle：
#       这一条是实测挖出来的坑——被记录过的路径之后再也导不进来，
#       表现为"索引恒空且零报错"
_IMPORT_FLAGS = ("-A", "-q", "-s", "-I", "-R")

# 命令行长度上限（Windows 32767）。留一半余量：路径可能很长。
_ARG_CHUNK_CHARS = 6000

# 索引入库的批大小。比上面那个小一个量级，理由见 `index_import`：批内没有
# 可观测的中间态，批越大"静默窗口"越长，超过宿主的 10 分钟看门狗就判卡死。
_INDEX_CHUNK_CHARS = 1500

# 假源的开关。**默认关**：假数据混进真库是最难排查的一类污染，
# 所以它必须是显式开、且只在自测/契约测试里开。
PROBE_ENV = "AIYA_BEETS_ENRICH_PROBE"

# Windows 下子进程不带这个 flag 会弹黑窗（父进程的 CREATE_NO_WINDOW 不继承）。
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


class SourceSpec:
    """一个增强源。`implemented=False` 的在 `want` 里被拒，只在 `fields` 里露面。

    为什么未接线的源不能"跑一遍返回空"：宿主会把 `empty` 缓存成"该源对这个
    文件不支持"，于是永远不会再试——批 5 接完线还得让用户手动清缓存。
    宁可现在就拒绝，把"没接"和"没数据"分开。
    """

    __slots__ = ("name", "implemented", "note")

    def __init__(self, name: str, implemented: bool, note: str):
        self.name = name
        self.implemented = implemented
        self.note = note


SOURCES: tuple[SourceSpec, ...] = (
    SourceSpec("probe", True, "自测源：按路径哈希造确定性字段，只在 AIYA_BEETS_ENRICH_PROBE=1 时可用"),
    SourceSpec("xtractor", False, "音频特征（bpm/danceability/mood）；需要 beets-xtractor + Essentia 二进制（批 5）"),
    SourceSpec("lastgenre", False, "流派推断（写 item.genre，不入文件）；需要联网（批 5）"),
)

# probe 声明的字段：与 xtractor 真实字段同名同形（docs/45 §3.2），
# 这样宿主那一侧的映射代码可以在假源上先把链路跑通。
#
# **7 个 mood_* 概率必须一起给**：宿主拿它们做 argmax 得到单列 `mood_label`
# （docs/45 §5.1）。只给 `mood_mirex` 的话，宿主那半边（提升列、按情绪筛选）
# 在假源上永远是空的——不是"没数据"，是"假源没喂到该喂的地方"，批 4 的
# 情绪筛选就没法在没有 Essentia 的机器上验。
_MOOD_CLASSES = (
    "mood_happy", "mood_sad", "mood_relaxed", "mood_aggressive",
    "mood_party", "mood_acoustic", "mood_electronic",
)

PROBE_FIELDS = ("bpm", "danceability", "average_loudness", "mood_mirex", *_MOOD_CLASSES)

_MOODS = (
    "mood_happy", "mood_sad", "mood_relaxed", "mood_aggressive",
    "mood_party", "mood_acoustic", "mood_electronic", "mood_mirex_cluster_1",
)


def probe_enabled() -> bool:
    return os.environ.get(PROBE_ENV, "").strip().lower() in ("1", "true", "yes")


def source_table() -> list[dict]:
    """给 `fields` 命令用：每个源一行，带三态。

    `state` 取值刻意与协议里 `enrichItem.status` 区分开（那是逐文件结果，
    这是"器能不能跑"），避免宿主把两个概念混成一个。
    """
    out = []
    for spec in SOURCES:
        if spec.name == "probe":
            state = "ready" if probe_enabled() else "disabled"
        else:
            state = "ready" if spec.implemented else "unwired"
        out.append({"name": spec.name, "state": state, "note": spec.note})
    return out


def _resolve_sources(want) -> list[SourceSpec]:
    """把请求里的 `want` 解析成源列表。未知/未接线一律拒绝，不静默忽略。"""
    known = {s.name: s for s in SOURCES}
    if not want:
        # 缺省 = 该家当前**真能跑**的源。索引家此刻就是 probe（若开）。
        usable = [s for s in SOURCES if s.implemented and (s.name != "probe" or probe_enabled())]
        if not usable:
            raise ProtocolError(
                E_INVALID_ARG, _STAGE,
                "这个家没有可用的增强源：先装 beets 并在索引家开启插件（本版可用的真人源在批 5 接入）",
            )
        return usable

    picked: list[SourceSpec] = []
    for raw in want:
        name = str(raw)
        spec = known.get(name)
        if spec is None:
            raise ProtocolError(
                E_INVALID_ARG, _STAGE,
                f"未知的增强源：{name!r}（可选：{', '.join(sorted(known))}）",
            )
        if not spec.implemented:
            raise ProtocolError(
                E_INVALID_ARG, _STAGE,
                f"增强源 {name!r} 尚未接线：{spec.note}",
            )
        if name == "probe" and not probe_enabled():
            raise ProtocolError(
                E_INVALID_ARG, _STAGE,
                f"增强源 {name!r} 是自测源，需要环境变量 {PROBE_ENV}=1",
            )
        if name not in [s.name for s in picked]:
            picked.append(spec)
    return picked


def _norm(path: str) -> str:
    """关联键：小写化。与宿主 `path_lower` 同一口径（Windows 大小写不敏感）。"""
    return path.replace("/", "\\").lower() if os.name == "nt" else path.lower()


def _probe_fields(path: str) -> dict:
    """确定性假字段：同一路径永远同样的值（便于断言幂等与续跑）。

    7 个情绪概率归一化成"和≈1 的分布"，形状与 essentia 的输出一致
    （各分类独立概率、量级 0.x），宿主的 argmax 才有可判的东西。
    """
    h = hashlib.sha256(_norm(path).encode("utf-8")).digest()
    raw = [h[4 + i] for i in range(len(_MOOD_CLASSES))]
    total = float(sum(raw)) or 1.0
    moods = {name: round(v / total, 4) for name, v in zip(_MOOD_CLASSES, raw)}
    out = {
        "bpm": float(60 + h[0] % 120),
        "danceability": round(h[1] / 255.0, 4),
        "average_loudness": round(h[2] / 255.0, 4),
        "mood_mirex": _MOODS[h[3] % len(_MOODS)],
    }
    out.update(moods)
    return out


def index_import(req_id: int, root: str | None, gen: int) -> None:
    """**只读入库**：把曲库根整批登记进索引家，不做任何分析（docs/49 §3.8）。

    为什么单独一条命令，而不是"让 `enrich` 的 want 为空"：`enrich` 里
    `want` 缺省的含义是"用这个家**当前能跑的源**"，与"我只要入库"是两件事。
    合成一个之后，"分析器没装"和"import 失败"会共用一个错误码——而它们的
    处置完全不同（前者等批 5，后者查文件权限）。实测：xtractor 未接线时整条
    `enrich` 被拒，索引库因此永远是空的，筛选页 0 命中。

    安全前提是配置里那三条 `import.write/copy/move: no`（cfg 硬钉 + 落盘后过闸）：
    这一步只让 beets 读文件、往**它自己的库**写行，不碰音频文件本身。
    """
    from ..beetslib import item_abs_path, open_library, scan_audio_files

    session = f"idx_{req_id:02d}"
    target = root or default_root()
    if not target:
        raise ProtocolError(
            E_INVALID_ARG, _STAGE,
            "没有曲库根：先在设置里选一个扫描目录（索引家对着它做只读分析）",
        )
    files = scan_audio_files([target])
    total = len(files)
    if total == 0:
        raise ProtocolError(E_INVALID_ARG, _STAGE, f"曲库根下没有音频文件：{target}")

    emit({"type": "progress", "session": session, "stage": "import",
          "current": 0, "total": total, "path": None})

    # 分批 import：单次命令行有长度上限（Windows 32767），而路径可以很长。
    #
    # 批大小刻意比 `_ARG_CHUNK_CHARS` 小得多：宿主有一条"10 分钟无输出就判卡死"
    # 的看门狗，而 `beet import` 是一次子进程调用、批内没有任何可观测的中间态。
    # 攒一大批的后果实测过——整库 1846 首被打进几十个大批，界面十分钟没动静，
    # 会话被宿主杀掉（日志：会话空闲超时），用户看到的是"入库到一半停了"。
    # 小批 + **每批开始前先报进度**，静默窗口就被压到单批耗时以内。
    chunk: list[str] = []
    size = 0
    done = 0
    for p in files:
        if cancel_gen() != gen:
            emit({"type": "cancelled", "session": session, "stage": "import"})
            return
        if chunk and size + len(p) + 3 > _INDEX_CHUNK_CHARS:
            emit({"type": "progress", "session": session, "stage": "import",
                  "current": done, "total": total, "path": chunk[0]})
            _run_import(chunk)
            done += len(chunk)
            chunk, size = [], 0
        chunk.append(p)
        size += len(p) + 3
    if chunk:
        emit({"type": "progress", "session": session, "stage": "import",
              "current": done, "total": total, "path": chunk[0]})
        _run_import(chunk)
        done += len(chunk)

    # 报"库里现在有多少条"而不是"我导了多少个文件"：import 会跳过已在库的、
    # 也可能失败，只有库里的行数才是用户接下来能查到的。
    lib = open_library()
    rows = list(lib.items())
    emit({"type": "progress", "session": session, "stage": "import",
          "current": total, "total": total, "path": None})
    emit({"type": "indexDone", "session": session, "stage": "import",
          "files": total, "items": len(rows),
          "roots": [os.path.normpath(item_abs_path(lib, it)) for it in rows[:3]]})


def default_root() -> str | None:
    """分析对象：宿主给的**曲库根**优先，否则 config 里的 `directory`。

    顺序不能反：没给曲库根时 config 的 directory 是占位 `scratch`（空目录），
    拿它当曲库根会得到"根下没有音频文件"这种看着像用户错了的报错。
    """
    from .. import cfg
    from ..beetslib import configured_directory

    return cfg.scan_root() or configured_directory()


def _import_paths(paths: list[str]) -> None:
    """补登：把路径导进当前家的库。失败不抛——逐个 status 由后面的读库决定。

    **必须先剔掉不存在的路径**：`beet import` 把 argv 里的路径一起校验，
    只要有一个不存在就 `error: no such file or directory` 并**整批不导**
    （实测：6 个真文件 + 1 个幽灵路径 → 库里 0 条）。宿主的清单来自数据库，
    文件在扫描与分析之间被删掉是正常事件，不剔就等于"一个文件没了，
    整批白跑"。

    **子进程的 stdout 必须掐掉**，这是防御性的：`serve()` 把 `sys.stdout`
    换成 stderr，那只管得住 Python 层的写；子进程继承的是**操作系统句柄**，
    绕过了这层重定向。实测 `beet import` 在库迁移时会往 stdout 打
    `Created database backup at: ...`，真插进管道就是一次协议污染。

    当前调用顺序里父进程先开了库，迁移发生在父进程（走 stderr），所以子进程
    通常无话可说——但"通常"不是契约。将来改成先导后开库、或者 beets 版本换了
    输出策略，这一行就是唯一的防线。
    """
    real = [p for p in paths if os.path.isfile(p)]
    missing = len(paths) - len(real)
    if missing:
        print(f"[aiya-beets] enrich 跳过 {missing} 个不存在的路径（不剔会毁掉整批）",
              file=sys.stderr)
    if not real:
        return
    chunk: list[str] = []
    size = 0
    for p in real:
        if chunk and size + len(p) + 3 > _ARG_CHUNK_CHARS:
            _run_import(chunk)
            chunk, size = [], 0
        chunk.append(p)
        size += len(p) + 3
    if chunk:
        _run_import(chunk)


def _run_import(chunk: list[str]) -> None:
    cmd = [sys.executable, "-m", "beets", "import", *_IMPORT_FLAGS, *chunk]
    try:
        subprocess.run(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=1800,
            check=False,
            **_NO_WINDOW,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        # 补登失败不是"整体失败"：没进库的路径会在后面以 skipped 逐条报出来，
        # 宿主据此保留工作清单，下次重试。
        print(f"[aiya-beets] enrich 补登失败：{e}", file=sys.stderr)


def _index_by_path(lib) -> dict[str, object]:
    """一次全表扫描建 path → item 索引。

    为什么不用 `lib.items(query)` 逐路径查：`path:` 匹配是**子串**语义，
    会连带命中同名前缀的其它文件（实测 docs/45 §3.6）。要精确只能靠
    `path::` 正则 + 转义，拼错一处就是静默 0 命中。索引一次更稳。
    """
    out: dict[str, object] = {}
    for item in lib.items():
        out[_norm(item.path.decode("utf-8", "replace"))] = item
    return out


def _detach(lib, paths: list[str]) -> None:
    """清旧：把清单里的项从库里摘掉（只清索引家，文件一个字节都不动）。

    用 `item.remove()` 而不是 `lib.remove(item)`：后者是 beets 1.x 的 API，
    2.x 已经删了（方法挂在模型上）。写错的结果不是报错而是**静默无效**
    ——`AttributeError` 被下面的 except 吞掉，`force` 看起来跑了、其实没摘，
    过期文件永远不会被重算。批 2 就是被这条咬过一次。
    """
    by_path = _index_by_path(lib)
    for p in paths:
        item = by_path.get(_norm(p))
        if item is None:
            continue
        try:
            item.remove()  # delete=False：只删库里的行，文件留着
        except Exception as e:  # noqa: BLE001 —— 单条失败不该中断整批
            print(f"[aiya-beets] enrich 清旧失败 {p}: {e}", file=sys.stderr)


def _apply_probe(item) -> tuple[str, dict]:
    """自测源：写 flexible 字段并回读。回读是刻意的——`status` 只认字段真出现，
    不认"我以为我写了"（真实源的教训见 docs/45 §3.7）。"""
    for k, v in _probe_fields(item.path.decode("utf-8", "replace")).items():
        item[k] = v
    item.store()
    got = {}
    for k in PROBE_FIELDS:
        v = item.get(k)
        if v is not None:
            got[k] = v
    return ("ok" if got else "empty"), got


def enrich(req_id: int, paths: list[str], want, force: bool, gen: int) -> None:
    """三段式主体：逐 (文件, 源) 发 `enrichItem`，最后 `enrichDone`。

    `paths` 是宿主算出来的工作清单（缺失 / 过期项）。工具不自己挑文件：
    "哪些该算"是宿主的判断（它才知道扫描根、size/mtime、上次分析时间）。
    """
    session = f"enr_{req_id:02d}"
    sources = _resolve_sources(want)

    paths = [str(p) for p in paths if str(p).strip()]
    total = len(paths)
    if total == 0:
        raise ProtocolError(E_INVALID_ARG, _STAGE, "paths 为空")

    def _progress(n: int) -> None:
        emit({
            "type": "progress", "session": session, "stage": _STAGE,
            "current": n, "total": total, "path": None,
        })

    # 心跳先发一条：后面补登是子进程，可能长时间没有输出。
    _progress(0)

    lib = open_library()
    if force:
        _detach(lib, paths)
    _import_paths(paths)
    # 重新开库：补登是子进程写的，当前连接看到的是旧页缓存
    lib = open_library()
    by_path = _index_by_path(lib)

    counts = {"ok": 0, "empty": 0, "failed": 0, "skipped": 0}
    ran: list[str] = []

    for i, p in enumerate(paths):
        if cancel_gen() != gen:
            # 取消是终态：不再发 enrichDone（两个终态会让客户端重复收尾）
            emit({"type": "cancelled", "session": session, "stage": _STAGE})
            return

        item = by_path.get(_norm(p))
        if item is None:
            for spec in sources:
                counts["skipped"] += 1
                emit({
                    "type": "enrichItem", "session": session, "path": p,
                    "source": spec.name, "status": "skipped", "fields": {},
                    "message": "补登后仍不在库里（文件可能不存在或不可读）",
                })
            continue

        for spec in sources:
            if spec.name not in ran:
                ran.append(spec.name)
            try:
                if spec.name == "probe":
                    status, fields = _apply_probe(item)
                    message = None
                else:  # pragma: no cover —— 未接线的源在 _resolve_sources 就被拒了
                    status, fields, message = "skipped", {}, f"源 {spec.name} 未接线"
            except Exception as e:  # noqa: BLE001 —— 单文件失败要逐条报，不能中断整批
                status, fields, message = "failed", {}, str(e)
            counts[status] += 1
            emit({
                "type": "enrichItem", "session": session, "path": p,
                "source": spec.name, "status": status,
                "fields": fields, "message": message,
            })

        # 进度按 5 首节流，最后一首必发。
        #
        # 为什么不是每首都发：一个文件有 N 个源就会发 N 条 `enrichItem`，
        # 再叠一条 `progress` 等于把管道流量翻倍，而宿主那边每条都要过一遍
        # 事件线程（管道只有 64 KB，反压的代价是"工具看起来卡住"）。
        # 为什么不是只发终态：400 首要跑十几秒到几分钟，进度条停在 0/N
        # 与"卡死"在用户眼里没有区别。
        if (i + 1) % PROGRESS_EVERY == 0 or i + 1 == total:
            _progress(i + 1)

    emit({
        "type": "enrichDone", "session": session,
        "total": sum(counts.values()),
        # 刻意叫 succeeded 而不是 ok：契约里"有 ok 就是应答信封"，
        # 事件带一个名为 ok 的键会被客户端当信封解析（批 2 踩过）
        "succeeded": counts["ok"], "empty": counts["empty"],
        "failed": counts["failed"], "skipped": counts["skipped"],
        "sources": ran,
    })
