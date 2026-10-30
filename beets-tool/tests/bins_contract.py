# -*- coding: utf-8 -*-
"""外置二进制的落盘契约（docs/53）：下载、双哈希、抽成员、probe、PATH 注入。

这一层最容易出"静默成功"：下到了错误页面、抽错了成员、exe 缺 DLL 跑不起来——
三种都会留下一个**看起来在**的文件，然后插件在真正调用时炸。所以门禁盯着
"坏输入必须报错，且不许留下半截文件"。

  [1] 体检形状：空目录 → 三个都 missing，字段齐全（UI 要靠这些键）
  [2] 抽成员：命中 1 个 OK；命中 0 个 / 2 个必须报错（不许"尽量找找"）
  [3] 坏载体：大小/哈希不符 → 报错，且 bin/ 里不留残骸
  [4] probe：正常 exe 认；退出非 0 且只有噪音的不认（缺 DLL 就是这个形状）
  [5] PATH 注入：前插、幂等，且注入后 `shutil.which` 能找到
  [6] 未知名字：`check` / `install_one` 都抛 KeyError
  [7] 包内 `bundle/bin`：免配置包自带的 exe 要能找到、且不用再下一次（docs/54）
  [8] `dest` 落点：打包脚本要把 exe 塞进包内目录，且 probe 不通就删干净
  [9] `--from-plugins`：宿主只报插件名，清单由这一侧推（docs/55）

不联网：全部用临时文件与本地 zip。真下载那条链在文档里记着实测（docs/53 §6）。
"""
import contextlib
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(TOOL, "src"))

from aiya_beets import bins, plugins  # noqa: E402

WORK = tempfile.mkdtemp(prefix="aiya-bins-")
BIN = os.path.join(WORK, "bin")
os.makedirs(BIN)
os.environ["AIYA_BEETS_BIN_DIR"] = BIN


def make_zip(path: str, members: dict) -> str:
    with zipfile.ZipFile(path, "w") as zf:
        for name, blob in members.items():
            zf.writestr(name, blob)
    return path


try:
    # ---- 1. 体检形状 ----
    rows = bins.check()
    assert [r["name"] for r in rows] == ["ffmpeg", "streaming_extractor_music", "fpcalc"], rows
    for r in rows:
        assert r["present"] is False and r["usable"] is False, r
        for key in ("target", "downloadBytes", "bytes", "license", "path", "version"):
            assert key in r, f"体检行缺键 {key}（UI 直接读它）"
        assert r["downloadBytes"] > 0 and r["bytes"] > 0, r
    print(f"[1] 体检形状 OK（{len(rows)} 项，全 missing）")

    # ---- 2. 抽成员的三种命中数 ----
    spec = plugins.binary("fpcalc")
    assert spec is not None
    one = make_zip(os.path.join(WORK, "one.zip"), {spec.member: b"exe-bytes"})
    assert bins.extract_member(Path(one), spec.member) == b"exe-bytes"
    zero = make_zip(os.path.join(WORK, "zero.zip"), {"无关/dir/fpcalc.exe.txt": b"x"})
    try:
        bins.extract_member(Path(zero), spec.member)
    except bins.BinError as e:
        assert "0" in str(e), e
    else:
        raise AssertionError("命中 0 个必须报错")
    two = make_zip(
        os.path.join(WORK, "two.zip"),
        {spec.member: b"a", "另一个/" + Path(spec.member).name: b"b"},
    )
    try:
        bins.extract_member(Path(two), "*.exe")
    except bins.BinError as e:
        assert "2" in str(e), e
    else:
        raise AssertionError("命中 2 个必须报错（不许随便挑一个）")
    # 目录项不算命中：有些包把目录名塞进 namelist
    with zipfile.ZipFile(os.path.join(WORK, "dironly.zip"), "w") as zf:
        zf.writestr("only/x.exe/", b"")
        zf.writestr("only/y.exe/", b"")
    try:
        bins.extract_member(Path(os.path.join(WORK, "dironly.zip")), "*.exe")
    except bins.BinError:
        pass
    else:
        raise AssertionError("只命中目录项时也必须报错")
    print("[2] 抽成员 OK（1 命中通过 / 0 与 2 命中报错 / 目录项不算）")

    # ---- 3. 坏载体：报错且不留残骸 ----
    junk = make_zip(os.path.join(WORK, "junk.zip"), {"whatever.exe": b"not-the-real-one"})
    try:
        bins.install_one("fpcalc", Path(junk))
    except bins.BinError as e:
        assert "大小" in str(e) or "sha256" in str(e), e
    else:
        raise AssertionError("坏载体必须报错")
    assert not (Path(BIN) / "fpcalc.exe").exists(), "失败后不许留下 exe"
    assert not list(Path(BIN).glob("*.part")), "失败后不许留下 .part"
    print("[3] 坏载体被拒 OK（且 bin/ 没有残骸）")

    # ---- 4. probe 的两种形状 ----
    real = Path(sys.executable)  # 拿本机解释器冒充"能跑的 exe"
    good = plugins.BinarySpec(
        name="x", target="x.exe", url="https://e/x.zip", carrier_size=1,
        carrier_sha256="0" * 64, member="x.exe", size=1, sha256="0" * 64,
        probe=("-c", "import sys; print('v1.2.3')"),
    )
    ok, version = bins.probe(real, good)
    assert ok and "v1.2.3" in (version or ""), (ok, version)
    dying = plugins.BinarySpec(
        name="y", target="y.exe", url="https://e/y.zip", carrier_size=1,
        carrier_sha256="0" * 64, member="y.exe", size=1, sha256="0" * 64,
        probe=("-c", "raise SystemExit(3)"),
    )
    ok2, _ = bins.probe(real, dying)
    assert not ok2, "非 0 退出 + 无 Usage 输出 = 跑不起来，必须不认"
    usage = plugins.BinarySpec(
        name="z", target="z.exe", url="https://e/z.zip", carrier_size=1,
        carrier_sha256="0" * 64, member="z.exe", size=1, sha256="0" * 64,
        probe=("-c", "import sys; print('Usage: z input output'); raise SystemExit(1)"),
        probe_allow_fail=True,
    )
    ok3, version3 = bins.probe(real, usage)
    assert ok3 and version3 is None, "允许失败的那类不该把用法提示当版本号"
    print("[4] probe OK（正常 / 缺 DLL 形状 / 用法提示）")

    # ---- 5. PATH 注入 ----
    before = os.environ.get("PATH", "")
    got = bins.inject_path()
    assert got is not None and str(got) == os.path.normpath(BIN) or str(got) == BIN, got
    after = os.environ.get("PATH", "")
    assert after.split(os.pathsep)[0] == str(got), "必须前插（系统里另有老版本时要优先我们的）"
    assert shutil.which("fpcalc") is None, "空目录里不该凭空找到"
    Path(BIN, "fpcalc.exe").write_bytes(b"stub")
    assert shutil.which("fpcalc") is not None, "注入后应当能找到"
    Path(BIN, "fpcalc.exe").unlink()
    bins.inject_path()
    assert os.environ["PATH"].count(str(got)) == 1, "重复调用不该重复插入"
    os.environ["PATH"] = before
    print("[5] PATH 注入 OK（前插 / 幂等 / 注入后 which 能找到）")

    # ---- 6. 未知名字 ----
    for call in (lambda: bins.check(["nope"]), lambda: bins.install_one("nope")):
        try:
            call()
        except KeyError:
            pass
        else:
            raise AssertionError("未知名字必须抛 KeyError")
    # 蓝图与实现不许漂：JSON 里声明的每个名字都要能查到
    for spec in plugins.BINARIES:
        assert plugins.binary(spec.name) is spec or plugins.binary(spec.name) == spec
        assert spec.carrier_sha256 and spec.sha256, spec.name
    print(f"[6] 未知名字报错 OK；蓝图 {len(plugins.BINARIES)} 条都能查到")
    # ---- 7. 免配置包自带 bin/ 也是一个查找来源（docs/54）----
    # 布局照宿主来：{data_root}/beets-tool/bin 与 {data_root}/beets-tool/bundle/bin
    root = Path(WORK, "root")
    pkg_bin = root / "beets-tool" / "bundle" / "bin"
    pkg_bin.mkdir(parents=True)
    src_exe = shutil.which("whoami")
    assert src_exe, "这台机器没有 whoami，本用例的『能跑的 exe』没法造"
    # 用 whoami 冒充提取器：无参调用退出 0 且有输出 —— 正好等于"probe 通得过"
    shutil.copyfile(src_exe, pkg_bin / "streaming_extractor_music.exe")

    saved_bin = os.environ.pop("AIYA_BEETS_BIN_DIR", None)
    saved_home = os.environ.get("AIYA_BEETS_HOME")
    os.environ["AIYA_BEETS_HOME"] = str(root / "beets")
    try:
        dirs = bins.bin_dirs()
        assert dirs == [root / "beets-tool" / "bin", pkg_bin], f"查找顺序不对：{dirs}"
        spec2 = plugins.binary("streaming_extractor_music")
        assert spec2 is not None
        found = bins.installed_path(spec2)
        assert found == pkg_bin / spec2.target, f"包内 exe 没被认出来：{found}"
        assert plugins.exe_path("streaming_extractor_music") == str(found), (
            "exe_path 必须也能看见包内那份（配置渲染靠它写 essentia_extractor）"
        )
        # PATH 也要带上：这样 followup 的 bins 下载不会把包里的当成"没有"
        got2 = bins.inject_path()
        assert got2 is not None and pkg_bin in bins.bin_dirs()
        assert str(pkg_bin) in os.environ["PATH"], "包内 bin 也该进 PATH"
        # 已经在包里 → ensure 不该再下一次（本地 bin/ 里不会多出一份）
        rows2 = bins.ensure(["streaming_extractor_music"])
        assert rows2 and rows2[0].get("skipped") is True, "包内已有的应跳过下载"
        assert not (root / "beets-tool" / "bin" / "streaming_extractor_music.exe").exists(), (
            "跳过时不该往 bin/ 里再写一份"
        )
        print("[7] 包内 bundle/bin OK（能查到 / exe_path 认 / 进 PATH / 不重复下载）")
    finally:
        os.environ["PATH"] = before
        if saved_bin is not None:
            os.environ["AIYA_BEETS_BIN_DIR"] = saved_bin
        if saved_home is None:
            os.environ.pop("AIYA_BEETS_HOME", None)
        else:
            os.environ["AIYA_BEETS_HOME"] = saved_home

    # ---- 8. `dest` 落点：打包脚本要把 exe 塞进包内目录 ----
    # 只验失败路径：坏载体下 dest 与 bin/ 都不许留残骸（"写错地方"在这里暴露）
    dest = Path(WORK, "workbin")
    try:
        bins.install_one("fpcalc", Path(junk), dest=dest)
    except bins.BinError:
        pass
    else:
        raise AssertionError("坏载体必须报错")
    assert not list(dest.glob("*")), f"dest 里不许有东西：{list(dest.glob('*'))}"
    assert not (Path(BIN) / "fpcalc.exe").exists(), "给了 dest 就不该写到 bin/"
    print("[8] dest 落点 OK（失败不留残骸 / 不串到 bin/）")

    # ---- 9. `--from-plugins`：宿主只报插件名，清单由这一侧算 ----
    # 宿主不再自己从内嵌表算 exe 并集（那是第二份派生，两边分家的症状是
    # "装了插件却没下工具"，而缺 ffmpeg 会让 beets 任何子命令都起不来）。
    def run_cli(argv):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = bins.main(argv)
        frames = [
            json.loads(line)
            for line in buf.getvalue().splitlines()
            if line.strip().startswith("{")
        ]
        return code, frames

    assert plugins.binaries_for(["replaygain", "lyrics"]) == ["ffmpeg"]
    assert plugins.binaries_for(["xtractor"]) == ["streaming_extractor_music"]
    assert plugins.binaries_for([]) == [], "空清单不该凭空推出工具"
    # 三个插件都要同一个工具时只下一次（去重保序）
    assert plugins.binaries_for(["replaygain", "convert", "chroma"]) == ["ffmpeg", "fpcalc"], \
        plugins.binaries_for(["replaygain", "convert", "chroma"])

    code, frames = run_cli(["--from-plugins", "lyrics"])
    assert code == 0 and frames[-1]["type"] == "done", frames
    assert frames[-1]["binaries"] == [], f"不需要工具时也要正常收尾：{frames}"
    code, frames = run_cli(["--from-plugins", "nope"])
    assert code == 2 and frames[-1]["type"] == "error", frames
    assert "不认识" in frames[-1]["message"], frames[-1]
    code, frames = run_cli(["--from-plugins", "lyrics", "--only", "ffmpeg"])
    assert code == 2 and "只能给一个" in frames[-1]["message"], frames[-1]
    print("[9] `--from-plugins` OK（按插件名推导 / 去重 / 空清单不算错 / 互斥 / 未知名字退出 2）")

finally:
    os.environ.pop("AIYA_BEETS_BIN_DIR", None)
    shutil.rmtree(WORK, ignore_errors=True)

print("ALL PASS")
