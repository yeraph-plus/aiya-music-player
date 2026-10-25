import type { AudioFormat } from './domain.js';

/**
 * 播放速率与音频规格的展示规则。
 *
 * 放在 contracts 里是因为三边都要用同一份：
 * - 状态机要按同样的网格夹取速率（否则 UI 显示 1.5×、实际 1.4×）
 * - 主进程校验 IPC 入参要用同一组上下限
 * - 渲染层渲染徽章要用同一张映射表
 *
 * 全是纯函数，不碰 DOM、不碰 fs，可以表驱动单测。
 */

/* ---------------- 速率 ---------------- */

export const AUDIO_RATE_MIN = 0.5;
export const AUDIO_RATE_MAX = 2;
export const AUDIO_RATE_STEP = 0.1;

/**
 * 夹取到 [0.5, 2.0] 并对齐到 0.1 网格。
 *
 * 用 `Math.round(v * 10) / 10` 而不是 `toFixed`：后者返回字符串，
 * 还要再转回来，且 `1.05 → "1.1"` 与 `1.04 → "1.0"` 的四舍五入方向
 * 在两处实现里容易写得不一致 —— 统一在这里算一次。
 */
export function clampRate(value: number): number {
  if (!Number.isFinite(value)) return 1;
  const snapped = Math.round(value * 10) / 10;
  return Math.min(AUDIO_RATE_MAX, Math.max(AUDIO_RATE_MIN, snapped));
}

/** 展示用：1 → "1.0×"，1.5 → "1.5×" */
export function formatRate(rate: number): string {
  return `${clampRate(rate).toFixed(1)}×`;
}

/** 相邻档位；到边界时返回原值（调用方据此判断"到顶了"） */
export function stepRate(rate: number, delta: number): number {
  return clampRate(clampRate(rate) + delta);
}

/* ---------------- 格式徽章 ---------------- */

/**
 * codec / container → 徽章文字。
 *
 * 优先看 codec：Opus 的容器也是 Ogg，但用户认知是 "OPUS"；
 * ALAC 的容器是 M4A，但标成 M4A 就看不出是不是无损了。
 *
 * 识别不了返回 null —— 显示 "?" 比不显示更糟。
 */
export function audioBadge(codec: string | null, container: string | null): string | null {
  const c = (codec ?? '').toLowerCase();
  const box = (container ?? '').toLowerCase();

  if (/alac/.test(c)) return 'ALAC';
  if (/flac/.test(c)) return 'FLAC';
  if (/opus/.test(c)) return 'OPUS';
  if (/vorbis/.test(c)) return 'OGG';
  // 'MPEG 1 Layer 3' / 'MPEG 2 Layer 3' / 'mp3'
  if (/mpeg.*layer\s*(3|iii)/.test(c) || /^mp3$/.test(c)) return 'MP3';
  if (/aac/.test(c)) return 'M4A';
  // PCM 的容器可能是 WAVE 也可能是 AIFF，两者都无损但不该混为一谈
  if (/pcm|wave|linear/.test(c)) return /aiff|aif/.test(box) ? 'AIFF' : 'WAV';
  if (/wma|windows media/.test(c)) return 'WMA';
  if (/dsd|dsf|dff/.test(c)) return 'DSD';

  // codec 缺失或不认识时退回容器
  if (/m4a|isom|mp4/.test(box)) return 'M4A';
  if (/flac/.test(box)) return 'FLAC';
  if (/ogg/.test(box)) return 'OGG';
  if (/aiff|aif/.test(box)) return 'AIFF';
  if (/wave|wav/.test(box)) return 'WAV';
  if (/mpeg/.test(box)) return 'MP3';
  if (/asf/.test(box)) return 'WMA';
  return null;
}

/**
 * 采样率展示：44100 → "44.1 kHz"，48000 → "48 kHz"，96000 → "96 kHz"。
 *
 * 不做 `toFixed(1)` 直出：48 kHz 会变成 "48.0 kHz"，看起来像凑数。
 */
export function formatSampleRate(hz: number): string {
  if (!Number.isFinite(hz) || hz <= 0) return '';
  const khz = Math.round((hz / 1000) * 10) / 10;
  return `${khz} kHz`;
}

/**
 * 主规格文字。
 *
 * 分支只看 `lossless`，**不看 `bitsPerSample`**：有损容器（AAC）也会报
 * `bitsPerSample = 16`，那是"解码器输出位深"，不是文件精度。按位深分支
 * 会把 m4a 标成 "44.1 kHz / 16 bit"，是错的。
 *
 * 无损也不显示码率：FLAC 的 bitrate 是文件平均码率（44.1k/16 实测约 900 kbps），
 * 拿它当音质展示会得出"FLAC 不如 320k MP3"的荒谬结论。
 *
 * **`bitrate` 的单位是 kbps，不是 bps**（Lofty 的 `audio_bitrate()` 文档写明
 * `(kbps)`，beets 的 `bitrate` 字段文档也写 "in kilobits per second"）。
 * 这里曾经写过 `/ 1000`，于是 320 kbps 的 MP3 显示成 "0 kbps"、
 * 48k/32bit 的 WAV 显示成 "3 kbps" —— 2026-10-25 实测发现。
 */
export function formatAudioSpec(audio: AudioFormat | null | undefined): string | null {
  if (!audio) return null;

  if (audio.lossless === true) {
    const parts: string[] = [];
    const rate = audio.sampleRate ? formatSampleRate(audio.sampleRate) : '';
    if (rate) parts.push(rate);
    if (audio.bitsPerSample && audio.bitsPerSample > 0) parts.push(`${audio.bitsPerSample} bit`);
    return parts.length > 0 ? parts.join(' / ') : null;
  }

  if (audio.bitrate && audio.bitrate > 0) return `${Math.round(audio.bitrate)} kbps`;
  // 有损但没有码率（部分 Ogg/Opus 变码率文件）：退回采样率，比空着强
  return audio.sampleRate ? formatSampleRate(audio.sampleRate) : null;
}

/**
 * 是否算 "Hi-Res"：无损且 24bit 以上或 88.2kHz 以上。
 * 只用来决定徽章是否高亮，不作为任何播放决策。
 */
export function isHiRes(audio: AudioFormat | null | undefined): boolean {
  if (!audio || audio.lossless !== true) return false;
  return (audio.bitsPerSample ?? 0) >= 24 || (audio.sampleRate ?? 0) >= 88_200;
}

/** 悬停提示：把播放条放不下的信息一次性给全 */
export function audioTooltip(audio: AudioFormat | null | undefined): string | null {
  if (!audio) return null;
  const parts: string[] = [];
  const badge = audioBadge(audio.codec, audio.container);
  if (badge) parts.push(badge);
  if (audio.codec && badge !== audio.codec) parts.push(audio.codec);
  const rate = audio.sampleRate ? formatSampleRate(audio.sampleRate) : '';
  if (rate) parts.push(rate);
  if (audio.bitsPerSample) parts.push(`${audio.bitsPerSample} bit`);
  if (audio.channels) parts.push(`${audio.channels} 声道`);
  parts.push(audio.lossless === true ? '无损' : audio.lossless === false ? '有损' : '未知');
  if (audio.bitrate && audio.bitrate > 0) {
    // 单位 kbps（见 formatAudioSpec 的注释），不需要再除 1000
    const kbps = Math.round(audio.bitrate);
    parts.push(audio.lossless === true ? `平均码率 ${kbps} kbps` : `${kbps} kbps`);
  }
  return parts.join(' · ');
}
