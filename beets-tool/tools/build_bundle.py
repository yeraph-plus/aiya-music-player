#!/usr/bin/env python
"""构建 B 轨道发布物：`aiya-beets-bundle-win-x64.zip`（免安装套件包）。

## 它解决什么

用户机器上**没有 Python**、也不该被要求去装一个。套件包把运行时整个搬过来：
自带解释器 + 标准库 + DLL + 依赖 + 我们的包。

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
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent          # beets-tool/tools
TOOL = HERE.parent                              # beets-tool
REPO = TOOL.parent
SRC = TOOL / "src"
VERSION = "0.1.0"


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
    sys.path.insert(0, str(SRC))
    try:
        from aiya_beets.protocol import PROTOCOL_VERSION

        return PROTOCOL_VERSION
    finally:
        sys.path.pop(0)


def _ignore(_dir: str, names: list[str]) -> set[str]:
    return {n for n in names if n == "__pycache__" or n.endswith(".pyc")}


# 自带解释器的版本：固定而不是"取最新"。发布物的可复现性优先于拿到 patch 更新。
EMBED_VERSION = "3.14.0"
EMBED_URL = (
    f"https://www.python.org/ftp/python/{EMBED_VERSION}"
    f"/python-{EMBED_VERSION}-embed-amd64.zip"
)
RUNTIME_DEFAULT = TOOL / ".runtime" / f"python-{EMBED_VERSION}-embed-amd64"


def ensure_runtime(runtime: Path) -> None:
    """准备自带解释器目录（缺什么补什么，已存在就复用）。

    这一步以前由播放器仓的 `tools/pack-beets-runtime.mjs` 干，M2 把它删了
    （运行时不再随播放器发布）。但**B 轨道要能重建**，所以这份责任搬到这里：
    下载官方 embeddable → 写 `_pth` → 装依赖。

    `_pth` 必须自己写：官方 embeddable 默认只列 `pythonXY.zip` 和 `.`，
    不含 `Lib/site-packages`，而 `_pth` 存在时 Python **忽略 PYTHONPATH**
    ——包放哪都 import 不到（这个坑 M2 踩过一次，写进 README 了）。
    """
    if (runtime / "python.exe").is_file() and (runtime / "Lib" / "site-packages").is_dir():
        return

    if not (runtime / "python.exe").is_file():
        runtime.parent.mkdir(parents=True, exist_ok=True)
        print(f"下载自带解释器 {EMBED_VERSION} → {runtime}")
        import urllib.request
        import zipfile

        tmp = runtime.parent / f"python-{EMBED_VERSION}-embed-amd64.zip"
        if not tmp.is_file():
            urllib.request.urlretrieve(EMBED_URL, tmp)
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


def install_deps(runtime: Path) -> None:
    """按 lock 把 beets 及其依赖装进 site-packages（已装过就跳过）。

    用**当前解释器**的 pip + `--target`：不依赖系统 pip，也不改动 dev venv。
    """
    site = runtime / "Lib" / "site-packages"
    if (site / "beets").is_dir():
        return
    req = TOOL / "requirements.lock"
    if not req.is_file():
        raise SystemExit(f"缺依赖清单：{req}")
    print(f"安装依赖 → {site}（按 {req.name}）")
    code = subprocess.call(
        [sys.executable, "-m", "pip", "install", "--disable-pip-version-check",
         "--target", str(site), "-r", str(req)],
        # pip 的 stdio 是我们的管道，编码由它自己决定；包名里有非 ASCII 时
        # cp1252 会崩（且报错信息本身也是乱码），统一给 UTF-8
        env={**os.environ, "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8"},
    )
    if code != 0:
        raise SystemExit(f"pip 安装依赖失败（退出码 {code}）")


def main() -> int:
    ap = argparse.ArgumentParser(description="打包 aiya-beets 免安装套件包")
    ap.add_argument("--runtime", default=str(RUNTIME_DEFAULT),
                    help="自带解释器目录（缺就自动下载官方 embeddable 并装依赖）")
    ap.add_argument("--out", default=str(TOOL / "dist" / "aiya-beets-bundle-win-x64.zip"))
    ap.add_argument("--work", default=str(TOOL / "dist" / "bundle"))
    ap.add_argument("--no-zip", action="store_true", help="只展开目录，不打包（调试用）")
    args = ap.parse_args()

    runtime = Path(args.runtime)
    ensure_runtime(runtime)
    install_deps(runtime)

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

    manifest = {
        "name": "aiya-beets",
        "version": VERSION,
        "protocolVersion": protocol_version(),
        "entry": "python.exe",
        "args": ["-m", "aiya_beets"],
        "note": "entry 相对解压根；播放器据此记下启动入口",
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
    print(f"自检通过：{probe.stdout.strip()}")

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
    print(f"OK  {out}  {out.stat().st_size / 2**20:.1f} MB")
    return 0


if __name__ == "__main__":
    _force_utf8_stdio()
    sys.exit(main())
