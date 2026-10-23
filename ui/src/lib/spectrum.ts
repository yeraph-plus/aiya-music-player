/**
 * 频谱分带的纯函数层：对数分带、聚合、平滑。
 *
 * 与 lib 层其它模块同一风格——不碰 DOM、不碰 AudioContext，
 * 输入输出都是可单测的数组。渲染（canvas / rAF）在组件里，
 * 取数节流与平滑状态在 AnalyserSource 里。
 *
 * 设计基准（ADR-037）：这是"可视化"而不是专业分析——
 * 32–64 个对数分带 + 峰值聚合 + 攻击/释放平滑，人眼可读即可。
 */

/** 单个分带在 FFT bin 轴上的范围 [start, end) */
export type BandRange = [number, number];

/**
 * 预计算对数分带的 bin 范围。
 *
 * 对数分布而不是线性：人耳对音高的感知是对数的，线性分带会让
 * 前几个分带挤满低音、高频段大面积空置。范围夹在 [minFreq, maxFreq]：
 * 40Hz 以下是次声/直流偏置，16kHz 以上人声与大多数乐器没有内容。
 *
 * 返回值保证严格递增且落在 [1, fftSize/2) 内（bin 0 是直流，跳过）。
 */
export function computeBandRanges(
  fftSize: number,
  sampleRate: number,
  bandCount: number,
  minFreq = 40,
  maxFreq = 16_000,
): BandRange[] {
  if (fftSize <= 0 || sampleRate <= 0 || bandCount <= 0) return [];

  const binHz = sampleRate / fftSize;
  const minBin = Math.max(1, Math.floor(minFreq / binHz));
  // Nyquist 之上的部分不存在；最后留至少 1 个 bin
  const maxBin = Math.min(Math.floor(fftSize / 2) - 1, Math.ceil(maxFreq / binHz));
  if (maxBin <= minBin) return [];

  const logMin = Math.log(minFreq);
  const logMax = Math.log(Math.min(maxFreq, maxBin * binHz));

  const ranges: BandRange[] = [];
  for (let band = 0; band < bandCount; band += 1) {
    const from = logMin + ((logMax - logMin) * band) / bandCount;
    const to = logMin + ((logMax - logMin) * (band + 1)) / bandCount;
    const startBin = Math.max(minBin, Math.floor(Math.exp(from) / binHz));
    const endBin = Math.max(startBin + 1, Math.min(maxBin, Math.ceil(Math.exp(to) / binHz)));
    ranges.push([startBin, endBin]);
  }
  return ranges;
}

/**
 * 把一帧 FFT 幅度（0–255）聚合成分带值（0–1）。
 *
 * 取带内**平均**而不是峰值：峰值在视觉上跳动过快，配合平滑反而糊；
 * 平均 + 攻击/释放平滑的组合观感最接近主流播放器。
 */
export function mapBands(fft: Uint8Array, ranges: readonly BandRange[]): Float32Array {
  const out = new Float32Array(ranges.length);
  for (let band = 0; band < ranges.length; band += 1) {
    const range = ranges[band];
    if (!range) continue;
    const [start, end] = range;
    let sum = 0;
    let count = 0;
    for (let bin = start; bin < end && bin < fft.length; bin += 1) {
      sum += fft[bin] as number;
      count += 1;
    }
    out[band] = count > 0 ? sum / count / 255 : 0;
  }
  return out;
}

/**
 * 攻击/释放平滑：上升快（attack 大，跟手），下降慢（release 小，拖尾）。
 *
 * 系数是"新值占比"：next = prev * (1 - k) + value * k。
 * 频谱的经典观感参数：上升一帧到位的 60%，下降一帧只落 10%。
 */
export function smoothBands(
  prev: Float32Array,
  value: Float32Array,
  attack = 0.6,
  release = 0.1,
): Float32Array {
  const out = new Float32Array(prev.length);
  for (let band = 0; band < prev.length; band += 1) {
    const target = value[band] ?? 0;
    const current = prev[band] ?? 0;
    const k = target > current ? attack : release;
    out[band] = current + (target - current) * k;
  }
  return out;
}
