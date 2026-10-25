"""生成解码测试用的二进制 fixture（Vorbis / ALAC / AIFF PCM）。

为什么要有这个脚本：symphonia 的解码分支只有真文件才能验。WAV 能手写
（tests/decode.rs 里就是手拼 RIFF 的），但 Vorbis 与 ALAC 是熵编码，手写不现实。

用法（只用 Python 三方包，本机不需要装 ffmpeg 本体）：

    python -m venv /tmp/fixenv
    /tmp/fixenv/Scripts/pip install soundfile av      # Windows；Linux/macOS 是 bin/
    /tmp/fixenv/Scripts/python gen_fixtures.py        # 在本目录执行

两个包各管一段，别指望一个顶俩（实测）：
- **soundfile**（libsndfile 1.2.2）→ OGG/Vorbis 与 AIFF。libsndfile 带 libvorbis，
  写出来的是标准 Vorbis。
- **PyAV**（wheel 自带 ffmpeg 库）→ ALAC。它的 ffmpeg 编译里**没有 libvorbis**
  （`UnknownCodecError: libvorbis`），原生 `vorbis` 编码器还是 experimental，
  所以 Vorbis 走 libsndfile。ALAC 是 ffmpeg 原生编码器，PyAV 直接可用。

产物（1kHz 正弦 0.3s + 静音 0.3s，44.1kHz 单声道，共 0.6s）：

    tone-vorbis.ogg   Vorbis，OGG 容器（symphonia: ogg demuxer + vorbis 解码）
    tone-alac.m4a     ALAC，MP4 容器（symphonia: isomp4 demuxer + alac 解码）
    tone-aac.m4a      AAC，MP4 容器（同上容器、另一条编码器路径）
    tone-mp3.mp3      MP3（LAME），验 gapless 的首尾 trim（编码器延迟 576 样本）
    tone-pcm.aiff     PCM 16bit 大端，AIFF 容器（symphonia: riff/aiff demuxer）

**为什么 AAC 也要一份**：MP4 容器（isomp4）压根不填 `codec_params.channels`，
通道数只藏在 magic cookie 里。ALAC 与 AAC 走同一个坑，而 AAC 是 Apple 生态
最常见的下载格式——只测 ALAC 不能代表 AAC 这条路。

这些文件**只给测试用**，不进运行时依赖。改信号形状或时长时，请同步改
tests/decode.rs 里的期望值（SR/DUR/TONE_MS 在那边也有一份）。
"""

import math
import sys
from pathlib import Path

import av
import numpy as np
import soundfile as sf

SR = 44_100
DUR = 0.6          # 秒
TONE_HZ = 1_000
TONE_AMP = 0.5     # 相对满刻度
TONE_MS = int(DUR * 1000 / 2)   # 正弦段时长：300ms，之后是静音

HERE = Path(__file__).resolve().parent


def signal() -> np.ndarray:
    """前半正弦、后半静音——两段语义让测试能验"seek 到静音区确实是静音"。"""
    n = int(SR * DUR)
    x = np.zeros(n, dtype="float32")
    for i in range(n // 2):
        x[i] = math.sin(2 * math.pi * TONE_HZ * i / SR) * TONE_AMP
    return x


def write_ogg_vorbis(x: np.ndarray, out: Path) -> None:
    sf.write(str(out), x, SR, format="OGG", subtype="VORBIS", compression_level=0.4)


def write_aiff_pcm(x: np.ndarray, out: Path) -> None:
    # AIFF 的默认字节序是大端（symphonia 的 pcm 解码器按 AIFF 规范处理）
    sf.write(str(out), x, SR, format="AIFF", subtype="PCM_16")


def write_mp4_encoded(x: np.ndarray, out: Path, codec: str) -> None:
    """用 PyAV 把 float32 PCM 喂给 ffmpeg 的编码器，装进 MP4 / MP3 容器。"""
    frame = av.AudioFrame.from_ndarray(
        x.reshape(1, -1), format="fltp", layout="mono"
    )
    frame.sample_rate = SR
    frame.pts = 0
    fmt = "mp3" if out.suffix == ".mp3" else "mp4"
    with av.open(str(out), "w", format=fmt) as o:
        ost = o.add_stream(codec, rate=SR, layout="mono")
        if out.suffix == ".mp3":
            ost.bit_rate = 128_000
        for pkt in ost.encode(frame):
            o.mux(pkt)
        for pkt in ost.encode(None):
            o.mux(pkt)


def main() -> int:
    x = signal()
    targets = [
        ("tone-vorbis.ogg", write_ogg_vorbis),
        ("tone-alac.m4a", lambda d, o: write_mp4_encoded(d, o, "alac")),
        ("tone-aac.m4a", lambda d, o: write_mp4_encoded(d, o, "aac")),
        ("tone-mp3.mp3", lambda d, o: write_mp4_encoded(d, o, "libmp3lame")),
        ("tone-pcm.aiff", write_aiff_pcm),
    ]
    for name, fn in targets:
        dst = HERE / name
        fn(x, dst)
        print(f"{name:16s} {dst.stat().st_size:>8,d} bytes")
        # 回读确认不是空壳（写坏了的 fixture 比没有更糟：测试会误判成解码器 bug）。
        # libsndfile 不认 MP4/MP3，这两类交给 PyAV 自检。
        if dst.suffix in (".m4a", ".mp3"):
            with av.open(str(dst)) as c:
                st = c.streams.audio[0]
                frames = st.frames or sum(1 for _ in c.decode(st))
                print(
                    f"  ↳ {st.codec_context.sample_rate} Hz × {len(st.layout.channels)}ch, "
                    f"{frames} frames, {c.format.name}/{st.codec_context.name}"
                )
        else:
            info = sf.info(str(dst))
            print(
                f"  ↳ {info.samplerate} Hz × {info.channels}ch, "
                f"{info.frames} frames, {info.format}/{info.subtype}"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
