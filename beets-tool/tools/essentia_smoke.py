"""Gaia/SVM 冒烟：正弦波 → 提取器（svm_models 指向包内 models/）→ 必须产出 highlevel。

CI 解包自检与本地验包共用。零外部依赖：wav 用 stdlib 现造、profile 现写。
断言只认三件事——12 件模型在位、提取器退出 0、highlevel.danceability 出现。
模型缺失或版本不配时 highlevel 整段消失（docs/50 §2.5 与 2026-10-31 实测），
"跑完了但字段全空"正是这道冒烟要堵的坑。
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import os
import struct
import subprocess
import sys
import tempfile
import wave

EXPECTED_MODELS = 12


def main() -> int:
    ap = argparse.ArgumentParser(description="验包：提取器 + SVM 模型必须产出 highlevel")
    ap.add_argument("--bundle", required=True, help="解开的套件包根（含 python.exe 与 bin/）")
    args = ap.parse_args()

    bundle = os.path.abspath(args.bundle)
    exe = os.path.join(bundle, "bin", "streaming_extractor_music.exe")
    if not os.path.isfile(exe):
        print(f"FAIL 找不到提取器：{exe}")
        return 1
    models = sorted(glob.glob(os.path.join(bundle, "bin", "models", "*.history")))
    if len(models) != EXPECTED_MODELS:
        print(f"FAIL 包内模型应 {EXPECTED_MODELS} 件，实际 {len(models)}：{models}")
        return 1

    work = tempfile.mkdtemp(prefix="essentia-smoke-")
    wav = os.path.join(work, "probe.wav")
    with wave.open(wav, "w") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(44100)
        w.writeframes(b"".join(
            struct.pack("<h", int(12000 * math.sin(2 * math.pi * 440 * i / 44100)))
            for i in range(44100 * 3)))

    profile = os.path.join(work, "profile.yaml")
    with open(profile, "w", encoding="utf-8", newline="\n") as f:
        f.write("highlevel:\n  compute: 1\n  svm_models:\n")
        for m in models:
            f.write(f"    - {m}\n")

    out = os.path.join(work, "out.json")
    r = subprocess.run([exe, wav, out, profile], capture_output=True, text=True,
                       errors="replace", timeout=300)
    if r.returncode != 0:
        print(f"FAIL 提取器退出码 {r.returncode}：{(r.stderr or r.stdout)[-500:]}")
        return 2
    if not os.path.exists(out):
        print("FAIL 提取器没有产出 json")
        return 2
    j = json.load(open(out, encoding="utf-8"))
    hl = j.get("highlevel", {})
    if "danceability" not in hl:
        print(f"FAIL highlevel 缺席或无 danceability（模型没被加载？）：{list(hl)[:8]}")
        return 3
    value = hl["danceability"].get("value") if isinstance(hl["danceability"], dict) else None
    print(f"OK highlevel 键数 {len(hl)}，danceability = {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
