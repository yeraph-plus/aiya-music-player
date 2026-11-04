#!/usr/bin/env python
"""SVM 模型自托管维护脚本：upf.edu 归档 → 12 件抽取 → 重打包 → 挂固定 release。

## 为什么存在

beets-xtractor 的高层特征（danceability / mood_* 等 12 件 SVM 模型）来自
essentia.upf.edu 的 beta5 归档——本机实测 ~17KB/s 且频繁断流，直接把慢源
转嫁给每一次插件安装。所以模型重打包后自托管在本仓库的固定 release
`svm-models-<VERSION>` 上，`plugins.json` 的 12 个 `svm-*` 条目从那里下载
（快、稳、可钉 sha256）。

## 与 GitHub Action 的关系

原载体是 `.github/workflows/svm-models-selfhost.yml`（自托管 runner），
2026-11-05 移入本维护链：模型不换版本就不需要跑，挂在 Actions 列表里是
零维护噪音。方法与 Action 逐步一致；只在**换模型版本**时由维护者本机跑：

    python tools/selfhost_svm_models.py --publish

## 12 件清单

= `aiya_beets.cfg.XTRACTOR_MODEL_FILES`（单一事实源，本脚本不复制清单）。
beta5 归档里另有 6 件（genre_dortmund/electronic/tzanetakis、ismir04_rhythm
等）不在配置模板内，不随包。

## 幂等

release 已存在则只追加资产（--clobber）。换模型版本：`--version v2.2_xxx`
重跑，plugins.json 的 url 随 tag 同步改。
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOOL = HERE.parent
SRC = TOOL / "src"
sys.path.insert(0, str(SRC))

from aiya_beets import cfg  # noqa: E402

REPO = "yeraph-plus/aiya-music-player"
UPF = "https://essentia.upf.edu/svm_models/essentia-extractor-svm_models-{ver}.tar.gz"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url: str, dest: Path) -> None:
    # 与 Action 的 curl --retry 3 同口径：慢源断流重试三次。
    for attempt in (1, 2, 3):
        try:
            with urllib.request.urlopen(url, timeout=120) as r, dest.open("wb") as f:
                while chunk := r.read(1 << 20):
                    f.write(chunk)
            return
        except OSError as e:
            if attempt == 3:
                raise SystemExit(f"下载失败（3 次）：{url}\n{e}")
            print(f"下载失败（第 {attempt} 次），重试：{e}")


def collect_models(archive: Path, out: Path) -> list[Path]:
    """从归档里抽 12 件铺平；缺任何一件都算失败（宁缺毋滥，配置门禁会响）。"""
    out.mkdir(parents=True, exist_ok=True)
    wanted = {f"{name}.history" for name in cfg.XTRACTOR_MODEL_FILES}
    found: dict[str, Path] = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            name = Path(member.name).name
            if name in wanted and member.isfile():
                src = tar.extractfile(member)
                if src is None:
                    continue
                (out / name).write_bytes(src.read())
                found[name] = out / name
    missing = sorted(wanted - found.keys())
    if missing:
        raise SystemExit(f"归档缺模型：{missing}")
    return [found[f"{name}.history"] for name in cfg.XTRACTOR_MODEL_FILES]


def pack(out_dir: Path, dest: Path) -> None:
    with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as zf:
        for f in sorted(out_dir.iterdir()):
            # zip 内路径 = ./<名>.history，与 Action 产物逐字节同构（entry 名对齐）
            zf.write(f, f"{f.name}")


def publish(tag: str, zip_path: Path) -> None:
    """挂到固定 release（推新删旧语义：--clobber 资产）。走 gh CLI。"""
    title = f"SVM models {tag}（beets-xtractor 高层特征，{len(cfg.XTRACTOR_MODEL_FILES)} 件）"
    notes = (
        f"Essentia Gaia SVM 模型（.history），供播放器随提取器分发与校验。"
        f"来源：essentia.upf.edu/svm_models（{tag} 归档重打包）。"
    )
    common = ["-R", REPO]
    view = subprocess.run(
        ["gh", "release", "view", tag, *common],
        capture_output=True, text=True,
    )
    if view.returncode != 0:
        subprocess.run(
            ["gh", "release", "create", tag, "--prerelease", "--title", title,
             "--notes", notes, *common],
            check=True,
        )
    subprocess.run(["gh", "release", "upload", tag, str(zip_path), "--clobber", *common],
                   check=True)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", default="v2.1_beta5",
                    help="upf 归档版本（同时决定 release tag：svm-models-<version>）")
    ap.add_argument("--publish", action="store_true",
                    help="打包后挂到固定 release（默认只打包并打印校验和，不发布）")
    ap.add_argument("--keep", action="store_true", help="保留工作目录（调试用）")
    args = ap.parse_args()

    tag = f"svm-models-{args.version}"
    work = Path(tempfile.mkdtemp(prefix="svm-selfhost-"))
    print(f"工作目录：{work}")
    archive = work / "svm_models.tar.gz"
    download(UPF.format(ver=args.version), archive)
    print(f"sha256({archive.name}): {sha256(archive)}")

    out = work / "out"
    models = collect_models(archive, out)
    for m in models:
        print(f"  {m.name}  sha256={sha256(m)}")

    zipped = work / "svm-models.zip"
    pack(out, zipped)
    print(f"sha256(svm-models.zip): {sha256(zipped)}")
    staged = work / tag / "svm-models.zip"
    staged.parent.mkdir(parents=True, exist_ok=True)
    zipped.replace(staged)
    print(f"打包完成：{staged}")

    if args.publish:
        publish(tag, staged)
        print(f"已上传到 release {tag}")
    else:
        print("未发布（加 --publish 挂到固定 release；或手动 gh release upload）")
    if not args.keep:
        print(f"清理工作目录：{work}")
        import shutil

        shutil.rmtree(work, ignore_errors=True)
    else:
        print(f"工作目录保留：{work}")


if __name__ == "__main__":
    main()
