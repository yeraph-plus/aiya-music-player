/**
 * 背景波纹地形图层（docs/46 P5 之二，2026-10-30）。
 *
 * 画法与正在播放页的 Canvas 2D 版同源（工作区 `JS波纹/` 的 MKVDB 2016 噪声
 * 壁纸）：**不画有起伏的面，只画一层层"等深线"**。真实水面在透视下，等深线
 * 越远越密、越近越疏，叠加起来就读成一片向观者铺开的水。这里把"高度"换成
 * 频谱曲线：48 个频带先经 `RidgeCurve` 平滑重采样成 256 点（与山形共用同一
 * 套数值规则），每一层取一个横向平移位，于是相邻层的峰谷错开，水面看起来是
 * 斜着淌的；只靠纵向平移的话，22 条线会叠成一排平行条纹。
 *
 * 为什么放到 WebGL 而不是照抄 2D 版：
 * 1. 背景已经有一个全尺寸 GL 舞台（`context.ts`）。再挂一块全屏 2D canvas
 *    会多一次合成事务，还要单独处理 DPR/尺寸——正是舞台要集中解决的脏活。
 * 2. 发光的代价。2D 版靠 `shadowBlur`，全窗口尺寸下是每层一次高斯；这里
 *    到线的像素距离在 fragment 里就有，光晕是它的指数衰减，一次 pass 画完。
 * 3. 层数可以更多。2D 版 22 层已经是性能上限，GL 里多几层几乎不影响。
 *
 * 与山形层（`ridge.ts`）的关系：同一个舞台、同一份频谱纹理思路，但**不是**
 * 同一个图层——山形是闭合填充（实形，over 合成），波纹是线（光，低 alpha）。
 * 两者由设置 `bgSpectrumStyle` 二选一，切形态时整层重建（不像光晕档位那样
 * 只改 uniform），因为几何结构完全不同。
 */

import {
  AddEquation,
  Color,
  CustomBlending,
  DataTexture,
  DataUtils,
  HalfFloatType,
  LinearFilter,
  Mesh,
  OneFactor,
  OneMinusSrcAlphaFactor,
  OrthographicCamera,
  PlaneGeometry,
  RedFormat,
  RepeatWrapping,
  Scene,
  ShaderMaterial,
} from 'three';
import { onThemeChange, themeColor } from '../../accent';
import type { RidgeGlow } from '../../settings';
import type { SpectrumSource } from '../../spectrum-source';
import type { Frame, GlLayer, GlStage } from '../context';
import { RidgeCurve } from '../ridge-curve';

/** 曲线重采样点数：2 的幂，NPOT 纹理所不必，但 256 在 2560px 宽上每段 10px */
const POINTS = 256;
const BANDS = 48;
/**
 * 层数。44 → 22 → 20 → 16 都试过，定版 22。
 *
 * 层数不是"越多越细"，而是**密度与可辨性的平衡**：层多到线距只剩十几像素时，
 * 叠加后读成一排平行横条（背景上尤其明显，因为线又长又直）；层太少近处又空。
 * 22 层 + pow 1.55 的透视 + 0.72 增益，是"密但不糊"的那一档。
 */
const LAYERS = 22;

/**
 * 两端环形化的窗宽（占点数的比例）。
 *
 * 波纹是**横向循环采样**的（层间平移会让采样点绕回起点），而曲线本身不是
 * 周期函数——带 0 和带 47 的值没有关系。直接用循环采样会在某个 x 处出现
 * 一条"突然断开"的竖缝，而且缝的位置随平移漂移，正好会漂到屏幕中间。
 *
 * 修法不是给 shader 打补丁，而是把曲线**做成环形的**：两端各 12% 用
 * smoothstep 窗压向一个共同的静息值，于是 h[0] == h[255] 且两侧斜率都趋近
 * 0，环绕采样天然连续。代价是最左/最右各一段水面偏平——但每层的平移位不同，
 * 压平段落在不同位置，叠加后只是"某些地方水面静一点"，不像断裂。
 */
const RING_WINDOW = 0.12;

/**
 * 光效档位（**与山形共用同一份设置** `ridgeGlow`，docs/46 §3.2）。
 *
 * 两层的几何不同（山形是实形 + 脊线，波纹是一排线），但档位的**语义必须一致**：
 * 同一个"锐光"在山形上是清晰脊线、在波纹上却毫无变化的话，用户会当成开关坏了。
 * 所以三档的判据是同一套：光的多少 + 边缘的锐度。
 *
 * - `soft` 柔雾：光晕**完全归零**（与山形 soft 的三层光归零对齐），线画粗、
 *   略压暗 → 只有形状，没有光；
 * - `balanced` 平衡：一层近场光晕（半径 9px），线宽原样；
 * - `neon` 锐光：线收细（×0.72）提亮（×1.18），光晕更强更聚（半径 6.4px）
 *   → 每条线的"线心"能单独看出来。
 *
 * 只改 uniform，不重建材质——切档不会闪黑帧（与 `RidgeLayer.setGlow` 同一约定）。
 */
const GLOW_PRESETS: Record<RidgeGlow, { line: number; width: number; halo: number; haloK: number }> =
  {
    soft: { line: 0.92, width: 1.35, halo: 0, haloK: 9.0 },
    balanced: { line: 1.0, width: 1.0, halo: 0.18, haloK: 9.0 },
    neon: { line: 1.18, width: 0.72, halo: 0.34, haloK: 6.4 },
  };

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const FRAG = /* glsl */ `
  varying vec2 vUv;
  uniform sampler2D uRamp;   // 256×1：横向高度曲线（0–1），S 方向已环形化
  uniform vec3 uColor;
  uniform float uTime;       // 秒：层间相位随它漂移（暂停时冻结）
  uniform float uEnergy;     // 整体能量（0–1）：安静段把浪压平
  uniform float uHeightPx;   // 画布高度（CSS px）：uv 距离 → 像素距离
  uniform float uHorizon;    // 地平线的 uv.y
  uniform float uSpan;       // 水面占的 uv 高度
  uniform float uGain;       // 整层总增益（主题相关的观感微调）
  uniform float uLine;       // 线亮度系数（光效档位）
  uniform float uWidth;      // 线宽系数（>1 更软、<1 更锐）
  uniform float uHalo;       // 近层光晕强度
  uniform float uHaloK;      // 光晕半径（px）：越大收得越快

  vec3 lin2srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), c));
  }

  void main() {
    // 地平线以上什么都不画。这一步省掉的是**一半像素 × 20 层**的循环，
    // 比任何片内优化都值——水面只在下半屏。
    if (vUv.y > uHorizon + 0.02) discard;

    // 左右各 8% 淡出 + 底部 6% 淡出。全窗口宽的水面直接顶到屏幕边缘会像
    // "一张图被裁断"，淡出后读成"水面延伸到画面之外"；底边同理——播放条
    // 压在上面，线被硬切会露出画布边界。
    float side = smoothstep(0.0, 0.08, vUv.x) * smoothstep(0.0, 0.08, 1.0 - vUv.x);
    side *= smoothstep(0.0, 0.06, vUv.y);
    if (side < 0.004) discard;

    float acc = 0.0;

    // 远（k=0，靠近地平线）→ 近（k=1，屏幕底）：远层窄、淡、密，近层宽、亮、疏。
    // 指数 1.55 是 2D 版实测出来的透视曲线：线性会让远处的层间距同样大，
    // 看着像等距横条而不是纵深。
    for (int l = 0; l < ${LAYERS}; l++) {
      float k = float(l) / float(${LAYERS - 1});
      float base = uHorizon - uSpan * pow(k, 1.55);

      // 横向包络：让波峰高度沿 x 缓缓起伏。不加这一项时每层是"等高、等距、
      // 只整体错位"的一族线，屏幕上读作平行条纹——真实水面的等深线不会
      // 处处等高，总有几处隆起的波包。**相位带层号**是关键：相邻层的波包
      // 位置错开，层与层才会交叉、交织，而不是一条曲线平移十七次。
      float envelope = 0.62 + 0.38 * sin(vUv.x * 6.2831 * 1.25 + float(l) * 1.7 + uTime * 0.4);
      float amp = uSpan * 0.34 * (0.15 + 0.85 * k) * envelope * uEnergy;

      // 两个方向的剔除：波形只向上鼓，所以上侧到波峰为止；下侧只留光晕半径。
      // 这一句让"这个像素根本不在这一层附近"的情况不花一次纹理采样。
      float dy = (vUv.y - base) * uHeightPx;
      if (dy < -26.0 || dy > amp * uHeightPx + 26.0) continue;

      // 带单位为 1、纹理方向为 1：平移量先除以 BANDS 再进 uv。
      // 每层 2.6 带、每秒 0.55 带：层间错位要大到"看得出斜淌"，但不能大到
      // 相邻层完全无关（那会散成噪声）；漂移速度则故意慢——背景要一直看着
      // 不烦，1%/s 的横向速度已经能感觉到"活着"。
      float off = (float(l) * 2.6 + uTime * 0.55) / ${BANDS}.0;
      float h = texture2D(uRamp, vec2(vUv.x + off, 0.5)).r;

      float d = (vUv.y - (base + h * amp)) * uHeightPx;
      float w = (0.8 + 1.2 * k) * uWidth;    // 线宽 px：近处更粗；档位再乘一次
      float line = exp(-(d * d) / (w * w));  // 高斯截面，天然抗锯齿
      // 光晕只给近层：远层是 1px 细线，光晕在那儿既看不见又白花成本
      float halo = exp(-abs(d) / uHaloK) * uHalo * step(0.55, k);

      // 远层近乎不可见（0.05），近层才真正"画出来"——这是纵深的本体，
      // 均匀 alpha 会让所有线一样重，读成格栅。上限压到 0.35 是因为背景
      // 后面有表格文字：线一旦比正文抢眼，整个背景就变成"条纹贴纸"。
      acc += line * (0.05 + 0.30 * pow(k, 1.4)) * uLine + halo;
    }

    float a =
      clamp(acc * uGain * side, 0.0, 1.0) *
      // 顶部（地平线）淡出：最远那层振幅最小，几乎是条水平直线，直接出现
      // 会在地平线处拉出一道亮边——看起来像"水面被一条横线封住"。
      // 往上 5% uv 内渐隐，水面就从暗处浮出来，没有边界可指。
      // （写成 1 - smoothstep(小, 大) 而不是 smoothstep(大, 小)：GLSL 规定
      //   edge0 >= edge1 时结果未定义，反着传在部分驱动上是随机值。）
      (1.0 - smoothstep(uHorizon - 0.05, uHorizon, vUv.y));
    if (a < 0.002) discard;

    vec3 col = lin2srgb(uColor);
    // 预乘 + over：波纹是"光"，但它压在固定渐变之上时用 over 才不会把
    // 亮色主题的浅底直接加到过曝（山形层同理，见 ridge.ts）。
    gl_FragColor = vec4(col * a, a);
  }
`;

let quad: PlaneGeometry | null = null;
function sharedQuad(): PlaneGeometry {
  if (!quad) quad = new PlaneGeometry(2, 2);
  return quad;
}

export class RippleLayer implements GlLayer {
  readonly name = 'ripple';

  private readonly scene = new Scene();
  private readonly camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly material: ShaderMaterial;
  private readonly texture: DataTexture;
  private readonly heights = new Uint16Array(POINTS);
  private readonly curve = new RidgeCurve({ bandCount: BANDS, points: POINTS });
  private readonly unsubscribeTheme: () => void;
  /**
   * 环形化之后的高度缓存。`RidgeCurve` 的输出每帧都在变，环形窗也要跟着重算
   * ——但窗本身只依赖下标，所以窗值预计算一次（`window`），每帧只做一次乘加。
   */
  private readonly window = new Float32Array(POINTS);
  /** 暂停时喂给曲线的一帧全 0：让曲线自己衰减回静息值，不留残峰 */
  private readonly zero = new Float32Array(BANDS);
  /** 暂停态：冻结时间 + 收平振幅 */
  private paused = false;
  /**
   * 收平/起振的过渡系数（0–1）。
   *
   * 暂停时直接跳 0 会"啪"一下没掉，像渲染挂了；这里用 70ms 时间常数做淡出，
   * 大约四帧收平——是"淡下去"，但仍然是一次性收干净，不停留旧形状。
   */
  private gate = 1;
  /** 图层自己的时钟：**只在播放时推进**，所以暂停后水面不再漂移 */
  private clock = 0;

  constructor(
    private readonly source: SpectrumSource,
    /** 初始光效档位；之后由 `setGlow` 驱动（组件用单独 effect，避免换档重建图层） */
    glow: RidgeGlow = 'balanced',
  ) {
    this.texture = new DataTexture(this.heights, POINTS, 1, RedFormat, HalfFloatType);
    this.texture.minFilter = LinearFilter;
    this.texture.magFilter = LinearFilter;
    this.texture.generateMipmaps = false;
    // S 方向必须重复：层间平移会让采样点绕回起点（见 RING_WINDOW）
    this.texture.wrapS = RepeatWrapping;
    this.texture.wrapT = RepeatWrapping;
    this.texture.needsUpdate = true;

    this.material = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      uniforms: {
        uRamp: { value: this.texture },
        uColor: { value: new Color('#7c9cff') },
        uTime: { value: 0 },
        uEnergy: { value: 0 },
        uHeightPx: { value: 800 },
        // 地平线在屏幕 66% 高度处：上半屏留给标题/列表，水面只铺下面三分之一强。
        // span 略微超出底边（0.34 - 0.36 = -0.02），由 shader 的底部淡出收掉——
        // 让最短的一层压到屏幕之外，近处才不会"突然停在地平线上方"。
        uHorizon: { value: 0.40 },
        uSpan: { value: 0.40 },
        // 总增益：背景是氛围，不能比列表文字更亮。0.7 是"看得到、但截图缩略时
        // 几乎注意不到"的量级（正在播放页的 2D 版是页内主角，不需要这个折让）。
        uGain: { value: 0.72 },
        // 光效档位的初值；构造函数末尾 `setGlow` 会按传入档位覆盖
        uLine: { value: 1 },
        uWidth: { value: 1 },
        uHalo: { value: 0.18 },
        uHaloK: { value: 9 },
      },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      blending: CustomBlending,
      blendSrc: OneFactor,
      blendDst: OneMinusSrcAlphaFactor,
      blendEquation: AddEquation,
    });

    const mesh = new Mesh(sharedQuad(), this.material);
    mesh.frustumCulled = false;
    this.scene.add(mesh);

    this.buildWindow();
    this.setGlow(glow);
    this.syncTheme();
    this.unsubscribeTheme = onThemeChange(() => this.syncTheme());
  }

  /** 换档位只改 uniform，不重建材质（切档不闪黑帧） */
  setGlow(glow: RidgeGlow): void {
    const p = GLOW_PRESETS[glow] ?? GLOW_PRESETS.balanced;
    const u = this.material.uniforms;
    u.uLine!.value = p.line;
    u.uWidth!.value = p.width;
    u.uHalo!.value = p.halo;
    u.uHaloK!.value = p.haloK;
  }

  /**
   * 播放/暂停。
   *
   * 暂停时的期望是"静止 + 收平"，不是"停在上一帧"：
   * 1. 冻结 `clock` ⇒ 层间平移不再推进，水面不漂；
   * 2. 喂全 0 + `gate` 淡出 ⇒ 振幅收平，**不保留暂停前的峰谷**。
   * 只冻时间不收平的话，暂停画面里会留一片凝固的浪；用户明确要的是"拉平"。
   */
  setPaused(paused: boolean): void {
    this.paused = paused;
  }

  /** 预计算环形窗：两端为 0（压向静息值），中间为 1（原样） */
  private buildWindow(): void {
    const k = Math.max(2, Math.round(POINTS * RING_WINDOW));
    const smooth = (x: number): number => x * x * (3 - 2 * x);
    for (let i = 0; i < POINTS; i += 1) {
      const head = smooth(Math.min(1, i / k));
      const tail = smooth(Math.min(1, (POINTS - 1 - i) / k));
      this.window[i] = head * tail;
    }
  }

  private syncTheme(): void {
    this.material.uniforms.uColor!.value = new Color(themeColor('--accent'));
  }

  resize(_width: number, height: number): void {
    this.material.uniforms.uHeightPx!.value = height;
  }

  draw(stage: GlStage, frame: Frame): void {
    this.advance(frame.dt);
    // 暂停时喂全 0：曲线自身的平滑会把残余峰谷衰减掉，配合 gate 收平，
    // 恢复播放时从 0 重新长起来，而不是"接上暂停前那一帧"
    this.curve.update(this.paused ? this.zero : this.source.frame(), frame.dt);
    this.upload();
    this.material.uniforms.uTime!.value = this.clock;
    stage.renderer.render(this.scene, this.camera);
  }

  /** 推进时钟与收平系数（暂停时时钟不动、gate 趋向 0） */
  private advance(dt: number): void {
    if (!this.paused) this.clock += dt;
    const target = this.paused ? 0 : 1;
    const k = 1 - Math.exp(-dt / 0.07);
    this.gate += (target - this.gate) * k;
    if (Math.abs(this.gate - target) < 0.002) this.gate = target;
  }

  /**
   * 曲线 → half-float 纹理。
   *
   * 三步：环形窗压平两端 → 顺带求均值当"静息值" → 写半浮点。静息值取
   * `mean * 0.5` 而不是 0：压到 0 会在屏幕两侧留下两段贴地的死水平线，
   * 和周围的波纹不在一个高度上，比断裂还显眼。
   */
  private upload(): void {
    const near = this.curve.near;
    const h = this.heights;
    const win = this.window;

    let sum = 0;
    for (let i = 0; i < POINTS; i += 1) sum += near[i] as number;
    const rest = (sum / POINTS) * 0.5;

    let energySum = 0;
    for (let i = 0; i < POINTS; i += 1) {
      const v = near[i] as number;
      energySum += v;
      const w = win[i] as number;
      // 乘 gate：暂停收平时连振幅一起收，否则线会平在 0.45 的能量下限上
      h[i] = DataUtils.toHalfFloat((v * w + rest * (1 - w)) * this.gate);
    }
    // 能量按曲线均值归一：均值 0.32 视为"满浪"（与 2D 版同一口径）
    this.material.uniforms.uEnergy!.value =
      Math.min(1, energySum / POINTS / 0.32) * this.gate;
    this.texture.needsUpdate = true;
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.texture.dispose();
    this.material.dispose();
  }
}
