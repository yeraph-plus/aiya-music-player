# -*- coding: utf-8 -*-
"""三个 smoke 共用的测试音源解析：有真音源就用，没有就现场合成。

为什么要有这个文件（2026-10-27 首次 CI 红的直接原因）：
    三个测试原本都写死 `D:\\MUSIC_Player\\test-music`，那是开发机的绝对路径。
    源码公开之后，CI runner 上既没有 D 盘也没有那三个 wav，`os.listdir` 直接
    抛 FileNotFoundError，整个 beets job 在第一步就失败。本地永远复现不出来，
    因为本地恰好有那个目录。

解析顺序（先命中先返回，只取前 `count` 个）：
    1. `$AIYA_TEST_MUSIC`
    2. `<repo>/test-music`
    3. `<repo>/../test-music`      ← 本仓开发机的布局（仓库与音源是兄弟目录）
    4. 合成到临时目录

合成物只保证"beets 能读、mutagen 能写 ID3、时长非零"：所有曲目名 / 艺人 /
专辑都由各测试的 `decision()` 显式给，不依赖源文件原有标签。所以合成音源
不会让断言变松——它替代的只是"一段能解码的音频"这个前提。
"""
import math
import os
import struct
import tempfile
import wave

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)          # beets-tool/
REPO = os.path.dirname(TOOL)          # 仓库根

# 三个音高，与开发机那三个 wav 同构，便于日志里对上号
_TONES = ((440, "中音测试 440Hz"), (220, "低音测试 220Hz"), (880, "高音测试 880Hz"))

_cache = None


def _wavs_in(directory):
    if not directory or not os.path.isdir(directory):
        return []
    return sorted(
        os.path.join(directory, name)
        for name in os.listdir(directory)
        if name.lower().endswith(".wav")
    )


def _synth(path, freq, seconds=1.0, rate=44100, amp=3000):
    """写一个单声道 16bit PCM 正弦波。够短（约 88 KB）以至于 12 组场景的复制也不慢。"""
    total = int(rate * seconds)
    with wave.open(path, "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(
            b"".join(
                struct.pack("<h", int(amp * math.sin(2.0 * math.pi * freq * i / rate)))
                for i in range(total)
            )
        )


def _generate(count):
    directory = tempfile.mkdtemp(prefix="aiya-testmusic-")
    paths = []
    for i in range(count):
        freq, label = _TONES[i % len(_TONES)]
        path = os.path.join(directory, f"{label}.wav")
        _synth(path, freq)
        paths.append(path)
    print(f"[i] 没有测试音源，已合成 {len(paths)} 个 wav 到 {directory}")
    return paths


def resolve(count=3):
    """返回 `count` 个可读 wav 的绝对路径。同进程内只解析一次。"""
    global _cache
    if _cache is not None and len(_cache) >= count:
        return _cache[:count]

    for candidate in (os.environ.get("AIYA_TEST_MUSIC"),
                      os.path.join(REPO, "test-music"),
                      os.path.join(os.path.dirname(REPO), "test-music")):
        found = _wavs_in(candidate)
        if len(found) >= count:
            _cache = found
            return found[:count]
        if found:
            # 目录在但不够数：说一句再往下走，否则"我明明放了文件"会变成谜案
            print(f"[i] {candidate} 只有 {len(found)} 个 wav，需要 {count} 个，继续找")

    _cache = _generate(count)
    return _cache
