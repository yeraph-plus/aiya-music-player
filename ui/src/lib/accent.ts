/**
 * 封面主色提取。
 *
 * 在渲染层做，不下放到主进程：`media://` 声明了 supportFetchAPI + corsEnabled，
 * fetch 出来的图是"同源干净"的，画到 canvas 后 getImageData 不会被污染，
 * 因此不需要为每种图片格式在主进程再引一套取色算法。
 *
 * 字节统一从 artwork-blob 取：Media Session 的封面也要同一份 Blob，
 * 两条路径共用一次网络/缓存请求。
 *
 * 失败一律返回 null（配色回落到默认强调色），取色永远不该影响播放。
 */

import { loadArtworkBlob } from './artwork-blob.js';

const cache = new Map<string, string | null>();
const SAMPLE = 24;

interface Hsl {
  h: number;
  s: number;
  l: number;
}

function rgbToHsl(r: number, g: number, b: number): Hsl {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };

  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === rn) h = ((gn - bn) / d + (gn < bn ? 6 : 0)) / 6;
  else if (max === gn) h = ((bn - rn) / d + 2) / 6;
  else h = ((rn - gn) / d + 4) / 6;
  return { h, s, l };
}

function hslToHex({ h, s, l }: Hsl): string {
  const hue2rgb = (p: number, q: number, t: number): number => {
    let value = t;
    if (value < 0) value += 1;
    if (value > 1) value -= 1;
    if (value < 1 / 6) return p + (q - p) * 6 * value;
    if (value < 1 / 2) return q;
    if (value < 2 / 3) return p + (q - p) * (2 / 3 - value) * 6;
    return p;
  };

  let r: number;
  let g: number;
  let b: number;
  if (s === 0) {
    r = l;
    g = l;
    b = l;
  } else {
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    r = hue2rgb(p, q, h + 1 / 3);
    g = hue2rgb(p, q, h);
    b = hue2rgb(p, q, h - 1 / 3);
  }

  const hex = (value: number): string =>
    Math.round(Math.min(1, Math.max(0, value)) * 255)
      .toString(16)
      .padStart(2, '0');
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

/** 让提取出来的颜色适合当强调色：够饱和、亮度居中，不会糊成一片灰 */
export function toAccent(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b);
  return hslToHex({
    h: hsl.h,
    s: Math.min(0.72, Math.max(0.42, hsl.s * 1.15)),
    l: Math.min(0.7, Math.max(0.52, hsl.l)),
  });
}

export async function accentForArtwork(url: string): Promise<string | null> {
  const cached = cache.get(url);
  if (cached !== undefined) return cached;

  let result: string | null = null;
  try {
    const blob = await loadArtworkBlob(url);
    if (!blob) throw new Error('artwork unavailable');
    const bitmap = await createImageBitmap(blob);
    const canvas = document.createElement('canvas');
    canvas.width = SAMPLE;
    canvas.height = SAMPLE;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (context) {
      context.drawImage(bitmap, 0, 0, SAMPLE, SAMPLE);
      const { data } = context.getImageData(0, 0, SAMPLE, SAMPLE);

      let weight = 0;
      let red = 0;
      let green = 0;
      let blue = 0;
      for (let i = 0; i < data.length; i += 4) {
        const alpha = data[i + 3] ?? 0;
        if (alpha < 128) continue;
        const r = data[i] ?? 0;
        const g = data[i + 1] ?? 0;
        const b = data[i + 2] ?? 0;
        const hsl = rgbToHsl(r, g, b);
        // 近黑近白与灰色不参与：它们决定不了封面的"气质"
        if (hsl.l < 0.12 || hsl.l > 0.9) continue;
        const w = 0.15 + hsl.s * hsl.s;
        red += r * w;
        green += g * w;
        blue += b * w;
        weight += w;
      }

      if (weight > 0) result = toAccent(red / weight, green / weight, blue / weight);
    }
    bitmap.close?.();
  } catch {
    result = null;
  }

  cache.set(url, result);
  return result;
}
