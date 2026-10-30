/**
 * 山形曲线：48 带 → 256 点的**纯 CPU 数值层**（docs/46 §3.2，P2）。
 *
 * 为什么不把 48 带直接送进 shader 让它线性插值：线性插值在 48 个采样点上
 * 就是折线——放大到 2560px 宽时，每个折点都能看见（docs/46 的 bench 截图
 * `shots/webgl.png` 里那种"锯齿山脊"就是它）。Catmull-Rom 是 C1 连续的，
 * 折角消失，且**不改变峰谷位置**（曲线穿过每个原始数据点）。
 *
 * 这个文件**刻意不含任何 GPU 代码**：时间平滑、伽马、远山倒序、振幅这些
 * 数值规则是原 Canvas 2D 实现里被用户认可的部分（docs/26 T4h 定稿），
 * 换光栅器不该顺手"优化"它们。抽成纯函数之后可以单独验证曲线本身，
 * 而不是只能靠看截图。
 */

/** 原实现：每帧向目标值靠拢 30%（60fps ≈ 90ms 时间常数） */
const SMOOTHING = 0.3;
/** 原实现：轻微伽马压低持续偏热的低频带，山形重心回到中频 */
const GAMMA = 1.2;
/** 近山全幅（顶部留白给渐变呼吸） */
const NEAR_AMP = 0.82;
/** 远山取倒序带 + 半幅：两层错开有纵深 */
const FAR_AMP = 0.5;

/**
 * 带域横向平滑权重（相邻带各占 22%，自己 56%）。
 *
 * 2026-10-30 加：低频几个带能量大、又各自独立跳动，同时冲高时会在山脊上
 * 留下又陡又窄的**孤立尖峰**——两支之间只有 5px 左右，放大后读作"竖线"，
 * 用户反馈成"低频出现垂直色带"。这是数据的形状问题，不是渲染精度问题
 * （换成 half-float 之后仍能看到）。
 *
 * 权重刻意温和：3 点 [0.22, 0.56, 0.22] 只削孤立峰（削掉约 44%），
 * 宽峰几乎不动，也不改变峰谷**位置**——山形的"起伏节奏"仍是原来的。
 */
const SIDE_W = 0.22;

export interface RidgeCurveOptions {
  bandCount: number;
  /** 重采样后的点数（默认 256） */
  points?: number;
}

/**
 * Catmull-Rom 插值（张力 0.5，即标准 CR）。
 * 端点用钳制索引（p0 取 p1、p3 取 p2）——不钳制会在两端甩出尖刺。
 */
function catmullRom(p0: number, p1: number, p2: number, p3: number, t: number): number {
  const t2 = t * t;
  const t3 = t2 * t;
  return (
    0.5 *
    (2 * p1 + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (-p0 + 3 * p1 - 3 * p2 + p3) * t3)
  );
}

/** 把 `src` 按 Catmull-Rom 重采样到 `dst` 的长度。两数组长度不变（复用缓冲） */
export function resampleCatmullRom(src: Float32Array, dst: Float32Array): void {
  const n = src.length;
  const m = dst.length;
  if (n === 0 || m === 0) return;
  if (n === 1 || m === 1) {
    dst.fill(src[0] as number);
    return;
  }

  const span = n - 1;
  for (let j = 0; j < m; j += 1) {
    const t = (j / (m - 1)) * span;
    const i1 = Math.min(span, Math.floor(t));
    const frac = t - i1;
    const i0 = i1 > 0 ? i1 - 1 : 0;
    const i2 = i1 + 1 <= span ? i1 + 1 : span;
    const i3 = i1 + 2 <= span ? i1 + 2 : span;
    dst[j] = catmullRom(src[i0] as number, src[i1] as number, src[i2] as number, src[i3] as number, frac);
  }
}

export class RidgeCurve {
  readonly near: Float32Array;
  readonly far: Float32Array;

  private readonly display: Float32Array;
  private readonly eased: Float32Array;
  /** 带域横向平滑的中间量（`eased` 的邻居加权，见 `SIDE_W`） */
  private readonly smoothed: Float32Array;
  /** 重采样输入缓冲：**必须与 `display` 分开**——`display` 是时间平滑的历史
   *  状态，被振幅缩放覆写过一次，下一帧的平滑就建立在错值上了（山会越走越扁） */
  private readonly shape: Float32Array;
  private readonly bandCount: number;

  constructor({ bandCount, points = 256 }: RidgeCurveOptions) {
    this.bandCount = bandCount;
    this.display = new Float32Array(bandCount);
    this.eased = new Float32Array(bandCount);
    this.smoothed = new Float32Array(bandCount);
    this.shape = new Float32Array(bandCount);
    this.near = new Float32Array(points);
    this.far = new Float32Array(points);
  }

  /**
   * 推进一帧。
   *
   * `dt` 用来把"每帧 0.3"换算成时间常数：`1 - (1-0.3)^(dt*60)`。
   * 60fps 下与原来的固定 0.3 **完全一致**（观感不变），但 30fps / 144Hz 上
   * 衰减速度也一致——原来的写法在 144Hz 屏上山会跟得更急、在低帧率下会拖尾。
   */
  update(bands: Float32Array | readonly number[], dt: number): void {
    const n = this.bandCount;
    const smooth = 1 - Math.pow(1 - SMOOTHING, Math.min(4, Math.max(0, dt)) * 60);

    for (let i = 0; i < n; i += 1) {
      // RemoteSpectrumSource.frame() 已归一化到 0–1，这里**不能再除 255**
      // （T4h 二次归一化把带值压到 0.4%，山成贴底平线）
      const v = Math.min(1, Math.max(0, (bands[i] as number) || 0));
      const prev = this.display[i] as number;
      const next = prev + (v - prev) * smooth;
      this.display[i] = next;
      this.eased[i] = Math.pow(next, GAMMA);
    }

    // 带域横向平滑：端点钳到自身，避免两端被"虚拟的 0"拉低（那会让山脊
    // 在最左/最右塌一个坑，看起来像截断）
    const center = 1 - SIDE_W * 2;
    for (let i = 0; i < n; i += 1) {
      const prev = this.eased[i > 0 ? i - 1 : 0] as number;
      const next = this.eased[i < n - 1 ? i + 1 : n - 1] as number;
      this.smoothed[i] = prev * SIDE_W + (this.eased[i] as number) * center + next * SIDE_W;
    }

    // 近山：正序 + 全幅；远山：倒序 + 半幅（两层错开才有纵深）
    for (let i = 0; i < n; i += 1) {
      this.shape[i] = Math.min(1, (this.smoothed[i] as number) * NEAR_AMP);
    }
    resampleCatmullRom(this.shape, this.near);

    for (let i = 0; i < n; i += 1) {
      this.shape[i] = Math.min(1, (this.smoothed[n - 1 - i] as number) * FAR_AMP);
    }
    resampleCatmullRom(this.shape, this.far);
  }
}
