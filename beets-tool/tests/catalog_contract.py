# -*- coding: utf-8 -*-
"""插件表契约（docs/51 §1）：`data/plugins.json` 是唯一事实源，五个消费方读同一份。

这一批没有功能，全是**结构前提**。表写错不会报错，只会让某处静默少一个插件、
或者把写盘插件放进索引家——所以门禁必须比使用方更严：

  [1] schema 合法：必需键齐全、类型对、name 唯一且小写
  [2] 派生视图与 JSON 一致：MARKET / DEFAULT / INDEX / BUNDLE 四路都不许自己发挥
  [3] 索引白名单的结构拒绝：「默认会写盘」的插件必须由 indexForced 压住 write:no
  [4] packages_for：未知名字必须抛错（静默跳过 = 勾了 A 装了 B）
  [5] bundle 插件不许带外部二进制依赖（否则"免配置"名不副实）
  [6] 负向九连：把表改坏（含 binaries 载体与引用），校验必须逐个变红

只读表，不写盘，不碰 beets 运行时。
"""
import copy
import json
import os
import sys
import tempfile
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(TOOL, "src"))

from aiya_beets import plugins  # noqa: E402

DOC = json.loads(plugins.catalog_text())

TOP_KEYS = {"schema", "beets", "profiles", "binaries", "plugins"}
ENTRY_KEYS = {
    "name", "title", "note", "builtin", "packages", "imports",
    "exe", "writes", "network", "bundle",
}
OPTIONAL_KEYS = {"indexForced"}

BINARY_KEYS = {
    "name", "target", "why", "carrier", "size", "sha256", "license", "vendor",
}
BINARY_OPTIONAL = {"probe", "probeAllowFail"}
CARRIER_KEYS = {"kind", "url", "size", "sha256", "member"}

HEX = set("0123456789abcdef")


def _is_sha256(value: object) -> bool:
    return isinstance(value, str) and len(value) == 64 and set(value) <= HEX


def validate(doc: dict) -> list[str]:
    """把不变量写成一组可读结论。返回空列表 = 合规。

    刻意不复用 `plugins.py` 的派生逻辑：门禁要是靠被测代码自己判自己，
    它坏了也会一起"通过"。这里只读原始 JSON。
    """
    errs: list[str] = []

    missing = TOP_KEYS - set(doc)
    if missing:
        return [f"顶层缺键：{sorted(missing)}"]
    if doc.get("schema") != 2:
        errs.append(f"schema 必须是 2，实际 {doc.get('schema')!r}")

    beets = doc.get("beets") or {}
    for k in ("min", "max"):
        if not isinstance(beets.get(k), str) or not beets.get(k):
            errs.append(f"beets.{k} 必须是非空字符串")

    items = doc.get("plugins")
    if not isinstance(items, list) or not items:
        return errs + ["plugins 必须是非空数组"]

    seen: list[str] = []
    for i, item in enumerate(items):
        if not isinstance(item, dict):
            errs.append(f"plugins[{i}] 不是对象")
            continue
        unknown = set(item) - ENTRY_KEYS - OPTIONAL_KEYS
        if unknown:
            errs.append(f"plugins[{i}] 有多余键：{sorted(unknown)}（打错字就会静默失效）")
        absent = ENTRY_KEYS - set(item)
        if absent:
            errs.append(f"plugins[{i}] 缺键：{sorted(absent)}")

        name = item.get("name")
        if not isinstance(name, str) or not name:
            errs.append(f"plugins[{i}].name 必须是非空字符串")
            continue
        if name != name.lower():
            errs.append(f"{name}: 插件名必须全小写（beetsplug.<name> 大小写敏感）")
        if name in seen:
            errs.append(f"{name}: 名字重复")
        seen.append(name)

        for key in ("packages", "imports", "exe"):
            val = item.get(key)
            if not isinstance(val, list) or not all(isinstance(x, str) and x for x in val):
                errs.append(f"{name}: {key} 必须是非空字符串数组（可以是空数组）")
        for key in ("builtin", "writes", "network", "bundle"):
            if not isinstance(item.get(key), bool):
                errs.append(f"{name}: {key} 必须是布尔")

        forced = item.get("indexForced")
        if forced is not None:
            if not isinstance(forced, dict):
                errs.append(f"{name}: indexForced 必须是对象")
            else:
                for k, v in forced.items():
                    if not isinstance(v, str):
                        errs.append(f"{name}: indexForced.{k} 必须是字符串（写进 YAML 原文）")

    # profiles 里出现的名字必须真的在表里——拼错一个字母就是"白名单收了个不存在的插件"
    for prof, names in (doc.get("profiles") or {}).items():
        if not isinstance(names, list):
            errs.append(f"profiles.{prof} 必须是数组")
            continue
        for n in names:
            if n not in seen:
                errs.append(f"profiles.{prof} 引用了不存在的插件：{n}")

    # 索引家的结构拒绝：默认会写盘的插件，只有显式 write: no 才准进
    by_name = {i["name"]: i for i in items if isinstance(i, dict) and "name" in i}
    for n in (doc.get("profiles") or {}).get("index", []):
        item = by_name.get(n)
        if item is None:
            continue  # 上面已经报过
        if item.get("indexForced") is None:
            errs.append(f"{n}: 进了 profiles.index 却没有 indexForced（无法压住默认参数）")
        elif item.get("writes") and item["indexForced"].get("write") != "no":
            errs.append(
                f"{n}: 默认会写文件，indexForced 必须含 write: no"
                f"（实测 xtractor 的 write 默认 yes，会静默改音频）"
            )

    # 免配置包的承诺是"解压即用"：带外部二进制的插件不该被预置
    for item in items:
        if isinstance(item, dict) and item.get("bundle") and item.get("exe"):
            errs.append(
                f"{item.get('name')}: 标了 bundle 却依赖外部命令 {item['exe']}"
                "（预置了也还是 missing_deps）"
            )

    # ---- binaries 段（docs/53）：外部二进制的载体与校验值 ----
    bins = doc.get("binaries")
    if not isinstance(bins, list) or not bins:
        errs.append("binaries 必须是非空数组（三个外部工具都在这儿声明）")
        return errs
    bin_names: list[str] = []
    for i, item in enumerate(bins):
        if not isinstance(item, dict):
            errs.append(f"binaries[{i}] 不是对象")
            continue
        unknown = set(item) - BINARY_KEYS - BINARY_OPTIONAL
        if unknown:
            errs.append(f"binaries[{i}] 有多余键：{sorted(unknown)}（打错字就会静默失效）")
        absent = BINARY_KEYS - set(item)
        if absent:
            errs.append(f"binaries[{i}] 缺键：{sorted(absent)}")
        name = item.get("name")
        if not isinstance(name, str) or not name:
            errs.append(f"binaries[{i}].name 必须是非空字符串")
            continue
        if name in bin_names:
            errs.append(f"{name}: 二进制名重复")
        bin_names.append(name)
        if not str(item.get("target", "")).endswith(".exe"):
            errs.append(f"{name}: target 必须是 bin/ 下的 .exe 文件名，实际 {item.get('target')!r}")
        if not _is_sha256(item.get("sha256")):
            errs.append(f"{name}: sha256 必须是 64 位十六进制")
        if not isinstance(item.get("size"), int) or item["size"] <= 0:
            errs.append(f"{name}: size 必须是正整数")
        for key in ("why", "license", "vendor"):
            if not isinstance(item.get(key), str) or not item.get(key):
                errs.append(f"{name}: {key} 必须是非空字符串")

        carrier = item.get("carrier")
        if not isinstance(carrier, dict):
            errs.append(f"{name}: carrier 必须是对象")
            continue
        unknown = set(carrier) - CARRIER_KEYS
        if unknown:
            errs.append(f"{name}: carrier 有多余键：{sorted(unknown)}")
        absent = CARRIER_KEYS - set(carrier)
        if absent:
            errs.append(f"{name}: carrier 缺键：{sorted(absent)}")
        if carrier.get("kind") != "zip":
            errs.append(f"{name}: carrier.kind 只支持 zip（轮子本身也是 zip）")
        url = carrier.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            errs.append(f"{name}: carrier.url 必须是 https 直链，实际 {url!r}")
        if not _is_sha256(carrier.get("sha256")):
            errs.append(f"{name}: carrier.sha256 必须是 64 位十六进制")
        if not isinstance(carrier.get("size"), int) or carrier["size"] <= 0:
            errs.append(f"{name}: carrier.size 必须是正整数")
        member = carrier.get("member")
        if not isinstance(member, str) or not member:
            errs.append(f"{name}: carrier.member 必须是非空字符串（载体内的路径/glob）")

    # 插件声明的 exe 必须能在 binaries 里找到：写错一个字母的后果是"安装器
    # 不知道要去下什么"，而插件照旧出现在目录里，用户点了也装不出来。
    for item in items:
        if not isinstance(item, dict):
            continue
        for dep in item.get("exe") or ():
            if dep not in bin_names:
                errs.append(f"{item.get('name')}: exe 引用了未声明的二进制 {dep}")

    for name in bin_names:
        if not any(
            isinstance(i, dict) and name in (i.get("exe") or ()) for i in items
        ):
            errs.append(f"{name}: 没有任何插件用到它（要么补插件，要么从表里删掉）")
    return errs


def main() -> int:
    # [1] 真实表必须合规
    errs = validate(DOC)
    assert not errs, "plugins.json 不合规：\n  " + "\n  ".join(errs)
    print(f"[1] schema 合法 OK：{len(DOC['plugins'])} 项")

    # [2] 派生视图与 JSON 一致
    assert len(plugins.MARKET_CATALOG) == len(DOC["plugins"]), "市场目录条数必须与表一致"
    assert [s.name for s in plugins.MARKET_CATALOG] == [i["name"] for i in DOC["plugins"]], (
        "市场目录顺序必须与表一致（UI 展示顺序）"
    )
    assert [s.name for s in plugins.DEFAULT_PLUGINS] == list(DOC["profiles"]["workbench"]), (
        "DEFAULT_PLUGINS 必须取自 profiles.workbench，不能另有一份"
    )
    assert [s.name for s in plugins.INDEX_PLUGINS] == [
        i["name"] for i in DOC["plugins"] if i.get("indexForced") is not None
    ], "INDEX_PLUGINS 必须等于『有 indexForced 的条目』"
    assert list(plugins.INDEX_DEFAULT_PLUGINS) == list(DOC["profiles"]["index"]), (
        "索引默认清单必须取自 profiles.index"
    )
    assert list(plugins.BUNDLE_PLUGINS) == [
        i["name"] for i in DOC["plugins"] if i["bundle"]
    ], "BUNDLE_PLUGINS 必须取自 bundle:true"
    assert [b.name for b in plugins.BINARIES] == [
        i["name"] for i in DOC["binaries"]
    ], "BINARIES 必须与 JSON 同序同名"
    print("[2] 派生视图与 JSON 一致 OK（MARKET/DEFAULT/INDEX/BUNDLE 四路 + BINARIES）")

    # [3] 索引白名单里没有"结构性写盘"的插件
    for spec in plugins.INDEX_PLUGINS:
        assert not spec.writes_files, f"{spec.name} 会写文件，不该出现在索引家白名单"
    # xtractor 是那条真实的教训：默认 write: yes，必须被压成 no
    xt = plugins.find_index_plugin("xtractor")
    assert xt is not None and ("write", "no") in xt.forced, "xtractor 必须压住 write: no"
    assert plugins.find_index_plugin("embedart") is None, "embedart 写文件，不该进索引白名单"
    print("[3] 索引白名单结构拒绝 OK（写盘插件不在，xtractor 的 write 被压住）")

    # [4] packages_for 的失败必须是响的
    assert plugins.packages_for(["lyrics"]) == ["beautifulsoup4>=4.12,<5"]
    assert plugins.packages_for(["lyrics", "lyrics"]) == ["beautifulsoup4>=4.12,<5"], "必须去重"
    assert plugins.packages_for(["scrub"]) == [], "不需要额外包的插件解析出空清单"
    try:
        plugins.packages_for(["lyric"])  # 少一个 s
    except KeyError as e:
        assert "lyric" in str(e), e
    else:
        raise AssertionError("未知插件名必须抛 KeyError，不能静默跳过")
    print("[4] packages_for OK（去重 / 空清单 / 未知名字抛错）")

    # [5] bundle 插件不许依赖外部二进制（validate 里已查，这里对真实清单再确认一次）
    for name in plugins.BUNDLE_PLUGINS:
        item = plugins.entry(name)
        assert not item.get("exe"), f"{name} 依赖外部命令，不该进免配置包"
    print(f"[5] bundle 预置清单 OK：{', '.join(plugins.BUNDLE_PLUGINS)}")

    # [5b] 外部二进制的解析：谁要什么、缺了算谁头上（docs/53）
    assert plugins.binaries_for(["replaygain"]) == ["ffmpeg"], "replaygain 只要 ffmpeg"
    assert plugins.binaries_for(["replaygain", "convert"]) == ["ffmpeg"], "必须去重"
    assert plugins.binaries_for(["lyrics"]) == [], "纯 Python 插件不涉及外部工具"
    assert plugins.binaries_for(["xtractor"]) == ["streaming_extractor_music"]
    try:
        plugins.binaries_for(["nope"])
    except KeyError:
        pass
    else:
        raise AssertionError("未知插件名必须抛 KeyError")
    # bin/ 里没有、PATH 上也没有 → 算缺；造一个占位文件 → 立刻算有
    with tempfile.TemporaryDirectory() as tmp:
        saved = os.environ.get("AIYA_BEETS_BIN_DIR")
        os.environ["AIYA_BEETS_BIN_DIR"] = tmp
        try:
            assert plugins.missing_binaries(["streaming_extractor_music"]), "空目录应该算缺"
            # 缺工具时：需要它的被剔掉，纯 Python 的照留（beets 会因此整体起不来）
            assert plugins.usable_plugins(["xtractor", "lyrics"]) == ["lyrics"], (
                "缺外部工具的插件必须被剔除，纯 Python 的照留"
            )
            Path(tmp, "streaming_extractor_music.exe").write_bytes(b"x")
            assert plugins.exe_path("streaming_extractor_music") is not None, "bin/ 里有了就要认"
            assert plugins.missing_binaries(["streaming_extractor_music"]) == []
            assert plugins.usable_plugins(["xtractor", "lyrics"]) == ["xtractor", "lyrics"], (
                "补齐之后必须自动恢复，顺序不变"
            )
            Path(tmp, "streaming_extractor_music.exe").unlink()
            assert plugins.usable_plugins(["xtractor"]) == [], "缺工具时不该留下 xtractor"
        finally:
            if saved is None:
                os.environ.pop("AIYA_BEETS_BIN_DIR", None)
            else:
                os.environ["AIYA_BEETS_BIN_DIR"] = saved
    print("[5b] binaries_for / exe_path / usable_plugins OK")

    # [6] 负向九连：把表改坏，校验必须变红（否则门禁是装饰）
    def broken(mutate) -> list[str]:
        doc = copy.deepcopy(DOC)
        mutate(doc)
        return validate(doc)

    cases = {
        "名字重复": lambda d: d["plugins"].append(copy.deepcopy(d["plugins"][0])),
        "白名单引用不存在的插件": lambda d: d["profiles"]["index"].append("nosuch"),
        "写盘插件没了 write: no": lambda d: d["plugins"][
            next(i for i, x in enumerate(d["plugins"]) if x["name"] == "xtractor")
        ]["indexForced"].pop("write"),
        "bundle 带外部命令": lambda d: d["plugins"][
            next(i for i, x in enumerate(d["plugins"]) if x["name"] == "lyrics")
        ].update({"exe": ["fpcalc"]}),
        "打错一个键": lambda d: d["plugins"][0].update({"nmae": "x"}),
        # docs/53：exe 引用与载体声明两边都得钉住
        "exe 引用不存在的二进制": lambda d: d["plugins"][0].update({"exe": ["nofmpeg"]}),
        "载体地址不是 https": lambda d: d["binaries"][0]["carrier"].update(
            {"url": "http://example.com/x.zip"}
        ),
        "sha256 写短了": lambda d: d["binaries"][0].update({"sha256": "abc123"}),
        "载体成员留空": lambda d: d["binaries"][0]["carrier"].update({"member": ""}),
        "声明了却没人用的二进制": lambda d: d["binaries"].append(
            {"name": "ghost", "target": "ghost.exe", "why": "x",
             "carrier": {"kind": "zip", "url": "https://e/x.zip", "size": 1,
                         "sha256": "0" * 64, "member": "ghost.exe"},
             "size": 1, "sha256": "0" * 64, "license": "MIT", "vendor": "x"}
        ),
    }
    for label, mutate in cases.items():
        got = broken(mutate)
        assert got, f"负向用例『{label}』没变红——这条门禁是装饰"
        print(f"    · {label} → 红：{got[0]}")

    print("ALL PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
