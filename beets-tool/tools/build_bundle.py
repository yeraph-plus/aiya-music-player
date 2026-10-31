#!/usr/bin/env python
"""构建 B 轨道发布物：`aiya-beets-bundle-win-x64.zip`（免配置套件包）。

## 它解决什么

用户机器上**没有 Python**、也不该被要求去装一个。套件包把运行时整个搬过来：
自带解释器 + 标准库 + DLL + 依赖 + 我们的包 + **用户勾选的插件依赖**。

"免配置"的含义是解压即全功能：`plugins.json` 里标了 `bundle: true` 的插件
（当前 lyrics / fetchart / mbsync / xtractor），它们的 pip 包已经躺在
`site-packages` 里。以前打出来的包是「beets + 依赖，零插件」，用户装完还得联网
pip 一次——那不是免配置，只是免装 Python。

`binaries[].bundle: true` 的外部工具（当前只有 Essentia 提取器，docs/54）也会被
下载、校验、抽出来塞进包里的 `bin/`：xtractor 没有那个 20MB 的 exe 就是**假开**
（渲染配置时会把它从 `plugins:` 里剔掉）。所以"预置 xtractor"这件事，真正的
工作在这一段。

## 为什么 exe 走 `bins` 的那套校验而不是自己下

`bins.fetch_blob` 是 size + sha256 + 成员唯一性三道闸的唯一实现。打包脚本要是
自己 `urlretrieve` 一次，就会出现"包里那个能跑、用户机下下来那个不一定"两条
没人对比过的路。这里只负责把它落到包内目录。

## 为什么可重定位（这是 B 轨道成立的前提）

Python 官方的 embeddable 发行版用 `python3xx._pth` 列出搜索路径（全部相对
解释器所在目录），`sys.prefix` 由 exe 位置推出 ⇒ 整个目录复制到任意路径都能跑。
这与 venv 相反：venv 的 `pyvenv.cfg` 里 `home` 是**绝对路径硬指针**，换个位置
就废（docs/29 批 C 实测，docs/33 §6.2）。

## 为什么 `_pth` 决定包放哪

`_pth` 存在时 Python **忽略 PYTHONPATH**（实测：只设 PYTHONPATH 时
`python.exe -m aiya_beets` 报 No module named aiya_beets）。所以我们的包必须
落在 `_pth` 列出的目录里 —— 当前是 `Lib\\site-packages`。
这也是本脚本不做"临时拼 sys.path"的原因：拼了在 embeddable 上根本不生效。

## 为什么用 uv 装依赖，而不是 pip（docs/51 §3.2）

同一件事 uv 快一个量级，且**解析目标解释器是认真的**：`--python <embed>/python.exe`
时它会去问那个解释器要 sysconfig，所以出来的是 cp314 的 wheel（实测
`numpy-2.5.4` tag = `cp314-cp314-win_amd64`），而不是打包机上的 3.13。
这正是"打出来的包到用户机 import 失败"这类事故的根因。

不引入中间 venv：venv 的作用是"能运行"，而 embeddable 本身就能跑。
中间建 venv 再拷 site-packages 只会多一次拷贝，并把 `pyvenv.cfg` 里那条
绝对路径带进来 —— 那恰好是 venv 不可重定位的原因，与免安装包的需求相反。

## 输出

    dist/aiya-beets-bundle-win-x64.zip
        python.exe  python3xx._pth  Lib/site-packages/{beets,aiya_beets,bs4,…}
        bin/streaming_extractor_music.exe
        manifest.json  LICENSE  THIRD-PARTY.md  LICENSES/AGPL-3.0.txt

`manifest.json` 里的 `plugins` / `packages` / `binaries` / `selfCheck` 是给宿主和
排障看的："这个包到底预置了什么、验过没有"必须能查，不能只凭构建日志。
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent          # beets-tool/tools
TOOL = HERE.parent                              # beets-tool
REPO = TOOL.parent
SRC = TOOL / "src"
sys.path.insert(0, str(SRC))

from aiya_beets import bins, plugins  # noqa: E402


def _force_utf8_stdio() -> None:
    """把这个脚本自己的 stdout/stderr 切成 UTF-8。

    Windows 上**管道**里的 stdout 按 locale 编码（英文 runner 是 cp1252），
    中文 print 直接 `UnicodeEncodeError` —— release 工作流实测崩在
    `下载自带解释器 … →` 那句。`PYTHONUTF8` 环境变量只在解释器启动前有效，
    脚本里设太晚（build_installer 以前那个 `os.environ.setdefault` 就是空操作）。
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


def protocol_version() -> int:
    """协议版本从源码读，不手抄。

    手抄过一次（3）——冻结成 v1 时忘了改这里，manifest 与 status 说的不是一个
    数字。凡是"同一事实存两份"的地方都会漂，能算就别写。
    """
    from aiya_beets.protocol import PROTOCOL_VERSION

    return PROTOCOL_VERSION


def package_version() -> str:
    """包版本同样从源码读。

    以前这里是硬编码的 `VERSION = "0.1.0"`，而 `aiya_beets.__version__` 早就
    是 0.2.0 —— 打出来的包在 manifest 里自称 0.1.0（docs/51 §0 实测）。
    """
    from aiya_beets import __version__

    return __version__


def _ignore(_dir: str, names: list[str]) -> set[str]:
    return {n for n in names if n == "__pycache__" or n.endswith(".pyc")}


# 自带解释器的版本：固定而不是"取最新"。发布物的可复现性优先于拿到 patch 更新。
# 2026-10-31 从 3.14.0 → 3.14.10：落后 10 个补丁版（通常含安全修复），且这个
# 解释器随免配置包分发给用户。哈希与 python.org 官方发布页逐字节核对过。
EMBED_VERSION = "3.14.10"
# 下载物的 sha256 钉扎（发布前审查 P2-7）：urlretrieve 无任何完整性校验，
# python.org 换版/被劫持会让发布物漂移。官方页 https://www.python.org/downloads/release/python-31410/
EMBED_SHA256 = "90905e64a840557d8c1bc4f5b74db3d7ee85e3ffb217611cac1c7813acea4159"
EMBED_URL = (
    f"https://www.python.org/ftp/python/{EMBED_VERSION}"
    f"/python-{EMBED_VERSION}-embed-amd64.zip"
)
RUNTIME_DEFAULT = TOOL / ".runtime" / f"python-{EMBED_VERSION}-embed-amd64"


def _sha256(path: Path) -> str:
    import hashlib

    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def ensure_runtime(runtime: Path) -> None:
    """准备自带解释器目录（缺什么补什么，已存在就复用）。

    这一步以前由播放器仓的 `tools/pack-beets-runtime.mjs` 干，M2 把它删了
    （运行时不再随播放器发布）。但**B 轨道要能重建**，所以这份责任搬到这里：
    下载官方 embeddable → 写 `_pth` → 装依赖。

    `_pth` 必须自己写：官方 embeddable 默认只列 `pythonXY.zip` 和 `.`，
    不含 `Lib/site-packages`，而 `_pth` 存在时 Python **忽略 PYTHONPATH**
    ——包放哪都 import 不到（这个坑 M2 踩过一次，写进 README 了）。
    """
    if not (runtime / "python.exe").is_file():
        runtime.parent.mkdir(parents=True, exist_ok=True)
        print(f"下载自带解释器 {EMBED_VERSION} → {runtime}")
        import urllib.request

        tmp = runtime.parent / f"python-{EMBED_VERSION}-embed-amd64.zip"
        if not tmp.is_file():
            urllib.request.urlretrieve(EMBED_URL, tmp)
        # 钉扎校验（EMBED_SHA256）：对不上就删，绝不带着来源不明的解释器进发布物。
        # 已存在但哈希不符的旧缓存同样拒绝——文件名相同不代表内容相同。
        actual = _sha256(tmp)
        if actual != EMBED_SHA256:
            tmp.unlink(missing_ok=True)
            raise SystemExit(
                f"embeddable 解释器 sha256 不符（实际 {actual}，期望 {EMBED_SHA256}）"
                "——下载可能被篡改或版本被上游替换，已删除。"
                "若确认要换版本：改 EMBED_VERSION 与 EMBED_SHA256（官方发布页核对）。",
            )
        with zipfile.ZipFile(tmp) as zf:
            zf.extractall(runtime)

    # _pth：把 site-packages 列进去，否则包 import 不到
    pth = sorted(runtime.glob("python3*._pth"))
    if not pth:
        raise SystemExit(f"没找到 _pth（{runtime}）：不像官方 embeddable 发行版")
    (runtime / "Lib" / "site-packages").mkdir(parents=True, exist_ok=True)
    tag = pth[0].stem.split("_")[0]          # python314
    pth[0].write_text(
        f"{tag}.zip\n.\nLib\\site-packages\nimport site\n",
        encoding="utf-8", newline="\n",
    )


def _importable(runtime: Path, modules: list[str]) -> list[str]:
    """用**包里的**解释器试 import，返回失败的模块名。"""
    bad: list[str] = []
    for mod in modules:
        got = subprocess.run(
            [str(runtime / "python.exe"), "-c", f"import {mod}"],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
        )
        if got.returncode != 0:
            bad.append(mod)
    return bad


def find_uv(explicit: str | None) -> str | None:
    """定位 uv。显式参数 > PATH > 无（回落 pip，但会打警告）。"""
    if explicit:
        return explicit
    return shutil.which("uv")


def install_deps(runtime: Path, extra: list[str], uv: str | None) -> None:
    """按 lock 装 beets 基础依赖 + 预置插件的额外包（已就位就跳过）。

    增量判断看两件事：`site-packages/beets` 在不在，以及预置插件的模块能不能
    import。只看前者会让"上一版包没有 lyrics"的缓存永远不补 bs4——那种包
    打出来是哑的，而构建日志一片绿。
    """
    site = runtime / "Lib" / "site-packages"
    need_mods = plugins.imports_for(list(plugins.BUNDLE_PLUGINS))
    if (site / "beets").is_dir() and not _importable(runtime, need_mods):
        print("依赖与预置插件已就位，跳过安装")
        return

    req = TOOL / "requirements.lock"
    if not req.is_file():
        raise SystemExit(f"缺依赖清单：{req}")

    # 装到**空目录**再合并，不能直接 `--target site`。
    #
    # 为什么（实测，docs/54 §5）：pip 的 `--target` 遇到已存在的顶层目录会整目录
    # 跳过，只打一条 `Target directory … already exists` 警告。beets 已经占着
    # `beetsplug/`，于是 `beets-xtractor` 的 `beetsplug/xtractor/` 永远进不去——
    # 构建日志一片绿（那些警告平时就在），只有最后一步的 import 自检会红，
    # 而且红得像是"包坏了"而不是"没装进去"。uv 的 `--target` 语义好一些，
    # 但两条路走同一个实现更省心：空目录装 + 合并，天然幂等，也不会留旧版本。
    staging = Path(tempfile.mkdtemp(prefix="aiya-deps-", dir=str(runtime.parent)))
    env = {**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8"}
    try:
        if uv:
            # --python 指 embeddable：让 uv 按它的版本/平台解析 wheel（cp314, win_amd64）
            # --link-mode=copy：`--target` 默认会 hardlink 到 uv 缓存，跨盘时 uv 自己
            # 也会回落 copy（还打一条 warning）。发布物应当自包含，统一 copy 更省事。
            cmd = [uv, "pip", "install", "--python", str(runtime / "python.exe"),
                   "--target", str(staging), "--link-mode=copy", "-r", str(req), *extra]
            print(f"安装依赖（uv）→ 合并进 {site}")
        else:
            # 没 uv 时回落 pip。功能一致，慢；开发者机器不一定装了 uv，
            # 但 CI 与正式发布必须走 uv（release-beets.yml 里有 setup-uv）。
            print("⚠ 没找到 uv，回落到 pip（CI 上不该走到这里）")
            cmd = [sys.executable, "-m", "pip", "install", "--disable-pip-version-check",
                   "--target", str(staging), "-r", str(req), *extra]

        code = subprocess.call(cmd, env=env)
        if code != 0:
            raise SystemExit(f"安装依赖失败（退出码 {code}）：{' '.join(cmd)}")
        site.mkdir(parents=True, exist_ok=True)
        shutil.copytree(staging, site, dirs_exist_ok=True, ignore=_ignore)
    finally:
        shutil.rmtree(staging, ignore_errors=True)


# 第三方许可全文（仓库内自带，构建时不联网抓：发布物要可复现）
THIRD_PARTY = TOOL / "third-party"


def bundle_binary_plan() -> list[str]:
    """包里要带哪些外置工具，并先掐掉"插件标了 bundle、exe 却没标"的组合。

    xtractor 是最容易踩的那条：插件标了 `bundle`、它要的 `streaming_extractor_music`
    没标，包打出来看着"预置了音频特征"，用户机上渲染配置时它被
    `plugins.usable_plugins` 剔掉——零报错、只是功能不在。所以这里当场失败。
    """
    wanted = list(plugins.BUNDLE_BINARIES)
    need: list[str] = []
    for name in plugins.BUNDLE_PLUGINS:
        for dep in (plugins.entry(name) or {}).get("exe", ()):
            if dep not in need:
                need.append(dep)
    missing = [n for n in need if n not in wanted]
    if missing:
        raise SystemExit(
            f"bundle 插件需要 {', '.join(missing)}，但它在 binaries 里没标 bundle:true"
            "（那样打出来的包会带着一个用不了的插件）"
        )
    return wanted


def stage_binaries(work: Path, names: list[str]) -> list[dict]:
    """下载 → 校验 → 落到包内 `bin/`，返回写进 manifest 的清单。

    落盘后 `bins.install_one` 会跑一次 probe：构建机上都跑不起来的 exe 不该被
    发出去（32 位 exe 靠 WoW64，构建机缺它就是一个"发出去才发现"的坑）。
    """
    if not names:
        return []
    print(f"准备外置工具 → {work / 'bin'}")
    rows = bins.stage(names, work / "bin", on_progress=lambda m: print(f"  {m}"))
    got = [r["name"] for r in rows]
    missing = [n for n in names if n not in got]
    if missing:
        raise SystemExit(f"外置工具没落盘：{', '.join(missing)}")
    out: list[dict] = []
    for row in rows:
        spec = plugins.binary(row["name"])
        assert spec is not None
        out.append({
            "name": spec.name,
            "target": spec.target,
            "bytes": row["bytes"],
            "sha256": spec.sha256,
            "version": row["version"],
            "license": spec.license,
            "vendor": spec.vendor,
            "source": spec.url,
        })
    return out


def _license_file_id(spdx: str) -> str | None:
    """`AGPL-3.0-or-later` → `AGPL-3.0`（许可全文的文件名）。

    `-or-later` 是"可以升版"的意思，附该版本正文即可。
    """
    if not spdx:
        return None
    return spdx.split("-or-later")[0].strip() or None


def write_third_party(work: Path, staged: list[dict]) -> None:
    """写清随包发布的第三方二进制来源与许可（docs/54）。

    ## 为什么这条不是可选项

    免配置包**转发别人的二进制**。AGPLv3 §4 要求转发时附许可副本、§6 要求提供
    对应源码的获取方式——只写一句 "AGPLv3" 不构成交付。

    落地三件事：
      1. `THIRD-PARTY.md`：名字、来源、许可、**精确的上游地址**、本包内 sha256
         （用户拿 sha256 就能自己核对"这份和上游是不是同一个东西"）；
      2. `LICENSES/<id>.txt`：许可全文（从仓库拷，不联网）；
      3. `manifest.json` 的 `binaries`：同一份事实的机器可读版本。

    这都不是法律意见：真要大规模对外分发，该让律师看一眼 §6 那种"未修改二进制的
    源码提供方式"够不够。眼下至少做到"愿意查的人查得到"。
    """
    if not staged:
        return
    ids: list[str] = []
    for row in staged:
        rid = _license_file_id(row["license"])
        if rid and rid not in ids:
            ids.append(rid)

    lines = [
        "# 随包发布的第三方二进制",
        "",
        "本套件包（`aiya-beets-bundle-win-x64.zip`）内含下列可执行程序。它们不是",
        "本项目源码，各自遵守原始许可；我们**原样转发，未做修改**。",
        "",
        "| 文件 | 来源 | 许可 | 本包内 sha256 |",
        "|---|---|---|---|",
    ]
    for row in staged:
        lines.append(
            f"| `bin/{row['target']}` | {row['vendor']} | {row['license']} | `{row['sha256']}` |"
        )
    lines += ["", "## 源码与上游", ""]
    for row in staged:
        lines += [
            f"### `bin/{row['target']}`",
            "",
            f"- 上游分发地址（就是本包内这份的出处）：{row['source']}",
            f"- 版本：{row['vendor']}",
            "- 我们未修改、未重打包，源码请按上面的上游项目对应 tag 获取；",
            "  若该地址失效，可凭本页的 sha256 在镜像站定位同一份二进制。",
            "",
        ]
    if ids:
        lines += ["## 许可全文", ""]
        lines += [f"- `LICENSES/{rid}.txt`" for rid in ids]
        lines.append("")
    (work / "THIRD-PARTY.md").write_text(
        "\n".join(lines) + "\n", encoding="utf-8", newline="\n"
    )

    for rid in ids:
        src = THIRD_PARTY / f"{rid}.txt"
        if not src.is_file():
            raise SystemExit(
                f"缺 {src}：{rid} 要求随包附许可全文（放进 beets-tool/third-party/）"
            )
        (work / "LICENSES").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, work / "LICENSES" / f"{rid}.txt")


def frozen_versions(site: Path) -> dict[str, str]:
    """从 dist-info 读出实际装上的版本（写进 manifest，便于复现与排障）。"""
    out: dict[str, str] = {}
    for meta in sorted(site.glob("*.dist-info/METADATA")):
        try:
            text = meta.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        name = version = None
        for line in text.splitlines():
            if line.startswith("Name: "):
                name = line[6:].strip()
            elif line.startswith("Version: "):
                version = line[9:].strip()
            if name and version:
                break
        if name and version:
            out[name] = version
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="打包 aiya-beets 免配置套件包")
    ap.add_argument("--runtime", default=str(RUNTIME_DEFAULT),
                    help="自带解释器目录（缺就自动下载官方 embeddable 并装依赖）")
    ap.add_argument("--out", default=str(TOOL / "dist" / "aiya-beets-bundle-win-x64.zip"))
    ap.add_argument("--work", default=str(TOOL / "dist" / "bundle"))
    ap.add_argument("--uv", default=None, help="uv 可执行文件（默认从 PATH 找）")
    ap.add_argument("--no-zip", action="store_true", help="只展开目录，不打包（调试用）")
    args = ap.parse_args()

    runtime = Path(args.runtime)
    ensure_runtime(runtime)

    plugin_names = list(plugins.BUNDLE_PLUGINS)
    extra = plugins.packages_for(plugin_names)
    uv = find_uv(args.uv)
    install_deps(runtime, extra, uv)

    # 装完立刻验：预置插件的模块必须真能 import。这是"免配置"的验收条件，
    # 不通过就别产出发布物（打出一个装不上插件的包比没包更糟）。
    bad = _importable(runtime, plugins.imports_for(plugin_names))
    if bad:
        print(f"预置插件依赖导入失败：{', '.join(bad)}", file=sys.stderr)
        return 1

    work = Path(args.work)
    if work.exists():
        shutil.rmtree(work)
    work.parent.mkdir(parents=True, exist_ok=True)

    print(f"展开运行时 → {work}")
    shutil.copytree(runtime, work, ignore=_ignore)

    # 包必须进 _pth 列出的目录（见模块注释）
    site = work / "Lib" / "site-packages"
    site.mkdir(parents=True, exist_ok=True)
    print("安装 aiya_beets 包 → Lib/site-packages")
    shutil.copytree(SRC / "aiya_beets", site / "aiya_beets", dirs_exist_ok=True, ignore=_ignore)

    # 外置工具（docs/54）：Essentia 提取器随包发。xtractor 没有它等于"预置了
    # 一个被渲染侧剔掉的插件"，所以这一段失败就整包失败。
    staged = stage_binaries(work, bundle_binary_plan())

    frozen = frozen_versions(site)
    manifest = {
        "name": "aiya-beets",
        "version": package_version(),
        "protocolVersion": protocol_version(),
        "entry": "python.exe",
        "args": ["-m", "aiya_beets"],
        "note": "entry 相对解压根；播放器据此记下启动入口",
        # 预置了什么、具体版本多少：宿主显示"开箱即有哪些能力"、
        # 用户报问题时我们也不用猜"你装的是哪版 bs4"
        "plugins": plugin_names,
        "pluginPackages": extra,
        "binaries": staged,
        "packages": frozen,
        "pluginsJsonSchema": plugins.catalog()["schema"],
    }
    (work / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n"
    )

    # 许可副本放在 zip 根（解压即见）+ 包内 data/ 各一份。
    # GPL-3.0 §4/§6 要求分发目标代码附许可全文；这个套件包是独立发布单元，
    # 用户机器上没有仓库，指 `../LICENSE` 等于没给。
    lic = TOOL / "LICENSE"
    if not lic.is_file():
        print("缺少 LICENSE：GPL 要求发布物内附许可全文", file=sys.stderr)
        return 1
    shutil.copyfile(lic, work / "LICENSE")
    write_third_party(work, staged)

    # 自检：用**包里的**解释器跑，证明可执行（这一步在老机器上会抓出缺 DLL）
    probe = subprocess.run(
        [str(work / "python.exe"), "-m", "aiya_beets", "version"],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        # 继承当前环境（缺 SYSTEMROOT 等基础变量时解释器起不来，看起来像"包坏了"）
        env={**os.environ, "PYTHONIOENCODING": "utf-8", "PYTHONUTF8": "1"},
    )
    if probe.returncode != 0 or not probe.stdout.strip():
        print(f"套件包自检失败：{probe.stdout} {probe.stderr[:400]}", file=sys.stderr)
        return 1
    print(f"自检通过：{probe.stdout.strip()}（预置 {', '.join(plugin_names)}"
          + (f" + bin/{', bin/'.join(r['target'] for r in staged)}" if staged else "")
          + "）")

    if args.no_zip:
        print(f"OK（未打包） {work}")
        return 0

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    print(f"压缩 → {out}（{sum(f.stat().st_size for f in work.rglob('*') if f.is_file()) / 2**20:.0f} MB 待压缩）")
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as zf:
        for path in sorted(work.rglob("*")):
            if path.is_file():
                zf.write(path, path.relative_to(work).as_posix())

    # 打包后再看一眼条目：宿主按 `bin/<target>` 去找，路径被过滤掉就等于没带
    if staged:
        with zipfile.ZipFile(out) as zf:
            names = set(zf.namelist())
        lost = [f"bin/{r['target']}" for r in staged if f"bin/{r['target']}" not in names]
        if lost:
            print(f"套件包里缺外置工具：{', '.join(lost)}", file=sys.stderr)
            return 1
    print(f"OK  {out}  {out.stat().st_size / 2**20:.1f} MB")
    return 0


if __name__ == "__main__":
    _force_utf8_stdio()
    sys.exit(main())
