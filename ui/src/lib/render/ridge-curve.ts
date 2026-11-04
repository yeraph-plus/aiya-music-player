/**
 * 山形曲线：48 带 → 256 点的纯 CPU 数值层。
 *
 * 不把 48 带直接送入 shader 做线性插值：线性插值在 48 个采样点即构成折线，
 * 放大到 2560px 宽时每个折点均可见（的 bench 截图 `shots/webgl.png`
 * 中的锯齿状脊线即为此）。Catmull-Rom 插值为 C1 连续，折角消失，且曲线穿过
 * 每个原始数据点，不改变峰谷位置。
 *
 * 本文件不含任何 GPU 代码：时间平滑、伽马、远山倒序、振幅等数值规则沿用
 * Canvas 2D 实现中已验证的参数，更换光栅器不应改动这些规则。
 * 抽为纯函数后可直接验证曲线本身，无需依赖截图比对。
 */

/** 时间平滑系数：每帧向目标值靠拢 30%（60fps 下约 90ms 时间常数） */
const SMOOTHING = 0.3;
/** 轻微伽马压低持续偏热的低频带，使山形重心回到中频 */
const GAMMA = 1.2;
/** 近山全幅（顶部留白给渐变呼吸） */
const NEAR_AMP = 0.82;
/** 远山取倒序带 + 半幅：两层错开有纵深 */
const FAR_AMP = 0.5;

/**
 * 带域横向平滑权重（相邻带各占 22%，自身占 56%）。
 *
 * 低频若干带能量大且各自独立跳动，同时冲高时会在脊线上留下陡而窄的孤立尖峰，
 * 相邻两支间距约 5px，放大后读作竖线（低频垂直色带）。这属于数据形状问题而非
 * 渲染精度问题：改用 half-float 后仍可观察到。
 *
 * 权重取值温和：3 点 [0.22, 0.56, 0.22] 仅削减孤立峰（约 44%），宽峰几乎不受
 * 影响，峰谷位置不变，起伏节奏与原曲线一致。
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

/**
 * 把重采样结果压回 [0,1]。
 *
 * Catmull-Rom 在"平台贴地"处会下冲：相邻两点都是 0、而外侧一点还有值时（安静段
 * 低频残余、其余带归零的典型形状），过渡段最低到 −0.125 × 外侧值。幅度再小也是
 * 负数，而波纹层对它取 `pow(curve, 0.6)`——负底数的幂在 GLSL 里未定义，那几个
 * 像素整列失效，画面上是一道贯穿水面的竖直断口。上界同理只是补齐值域：山形与
 * 波纹都按 0–1 的高度消费这条曲线。
 */
function clampCurve(values: Float32Array): void {
  for (let i = 0; i < values.length; i += 1) {
    const v = values[i] as number;
    values[i] = v < 0 ? 0 : v > 1 ? 1 : v;
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
   * `dt` 把每帧系数换算为时间常数：`1 - (1-0.3)^(dt*60)`。60fps 下与固定 0.3
   * 完全等价，30fps 与 144Hz 下衰减速度亦一致。使用固定系数时，高刷新率屏幕上
   * 跟随后端过快，低帧率下则产生拖尾。
   */
  update(bands: Float32Array | readonly number[], dt: number): void {
    const n = this.bandCount;
    const smooth = 1 - Math.pow(1 - SMOOTHING, Math.min(4, Math.max(0, dt)) * 60);

    for (let i = 0; i < n; i += 1) {
      // RemoteSpectrumSource.frame() 已归一化到 0–1，此处不得再次除以 255：
      // 二次归一化会把带值压到 0.4%，山形贴底成一条平线
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
    clampCurve(this.near);

    for (let i = 0; i < n; i += 1) {
      this.shape[i] = Math.min(1, (this.smoothed[n - 1 - i] as number) * FAR_AMP);
    }
    resampleCatmullRom(this.shape, this.far);
    clampCurve(this.far);
  }
}
