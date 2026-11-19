import { resampleCatmullRom } from './ridge-curve';

const BANDS = 48;
const RADIUS = 6;
const SIGMA = 2.2;
const TAU = 0.24;
const kernel = new Float32Array(RADIUS * 2 + 1);
let weight = 0;
for (let i = -RADIUS; i <= RADIUS; i += 1) {
  const value = Math.exp(-(i * i) / (2 * SIGMA * SIGMA));
  kernel[i + RADIUS] = value;
  weight += value;
}
for (let i = 0; i < kernel.length; i += 1) kernel[i] = kernel[i]! / weight;

function filter(input: Float32Array, output: Float32Array): void {
  for (let i = 0; i < BANDS; i += 1) {
    let sum = 0;
    for (let j = -RADIUS; j <= RADIUS; j += 1) {
      sum += input[Math.max(0, Math.min(BANDS - 1, i + j))]! * kernel[j + RADIUS]!;
    }
    output[i] = sum;
  }
}

/** 240ms 时间平滑、两遍 Gaussian 带间滤波与 256 点插值；全部缓冲就地复用。 */
export class SoftSpectrum {
  readonly values = new Float32Array(256);
  private readonly history = new Float32Array(BANDS);
  private readonly scratch = new Float32Array(BANDS);
  private readonly filtered = new Float32Array(BANDS);

  update(input: Float32Array, dt: number): Float32Array {
    const seconds = Number.isFinite(dt) ? Math.max(0, Math.min(0.1, dt)) : 0;
    const k = 1 - Math.exp(-seconds / TAU);
    for (let i = 0; i < BANDS; i += 1) {
      const raw = input[i] ?? 0;
      const value = Number.isFinite(raw) ? Math.max(0, Math.min(1, raw)) : 0;
      this.history[i] += (value - this.history[i]!) * k;
    }
    filter(this.history, this.scratch);
    filter(this.scratch, this.filtered);
    resampleCatmullRom(this.filtered, this.values);
    for (let i = 0; i < this.values.length; i += 1) {
      this.values[i] = Math.max(0, Math.min(1, this.values[i]!));
    }
    return this.values;
  }
}
