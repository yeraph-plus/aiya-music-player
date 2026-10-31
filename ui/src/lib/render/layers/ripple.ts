/**
 * 背景波纹地形图层（docs/46 P5）。
 *
 * 画法：不绘制有起伏的面，只绘制一层层等深线。真实水面在透视下等深线远密近疏，
 * 叠加后读作一片向观者铺开的水面。此处把"高度"替换为频谱曲线：48 个频带先经
 * `RidgeCurve` 平滑重采样为 256 点（与山形共用同一套数值规则），每层取一个横向
 * 平移位，使相邻层峰谷错开，水面呈斜向流动；仅做纵向平移时，22 条线会叠成平行
 * 条纹。
 *
 * 每层高度为底噪与频谱两项之和：频谱项乘能量门控（暂停时归零），底噪为一维值
 * 噪声，按层号错开相位且不归零。因此静音与暂停时呈现为缓慢流动的细微波纹，而
 * 非 22 条等高直线；若仅保留频谱项，安静画面会被判为渲染失败。
 *
 * 在 WebGL 而非 Canvas 2D 中实现的原因：
 * 1. 背景已有全尺寸 GL 舞台（`context.ts`）。另挂一块全屏 2D canvas 会增加一次
 *    合成事务，并需单独处理 DPR 与尺寸，而这些问题正是舞台集中解决的。
 * 2. 发光成本。2D 版依赖 `shadowBlur`，全窗口尺寸下每层需一次高斯；此处像素到
 *    线的距离在 fragment 中可得，光晕为其指数衰减，一次 pass 完成。
 * 3. 层数可更多。2D 版 22 层已接近性能上限，GL 下增加层数影响很小。
 *
 * 与山形层（`ridge.ts`）共用舞台与频谱纹理思路，但属不同图层：山形为闭合填充
 * （实形，over 合成），波纹为线（低 alpha）。两者由设置 `bgSpectrumStyle` 二选
 * 一，切换形态时整层重建，因为几何结构完全不同，无法仅修改 uniform。
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
 * 层数，取值 22。
 *
 * 层数决定密度与可辨性的平衡，并非越多越细：线距缩到十余像素时，叠加后读作
 * 一排平行横条（背景上尤明显，因线长而直）；层数过少则近处留白。22 层配合
 * pow 1.55 的透视与 0.72 增益，是密度足够且不模糊的取值。
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
 * 光效档位（与山形共用同一份设置 `ridgeGlow`，docs/46 §3.2）。
 *
 * 两层几何不同（山形为实形加脊线，波纹为一排线），但档位语义必须一致：同一档位
 * 在山形上表现为清晰脊线、在波纹上毫无变化，会被判为开关失效。因此三档使用同一
 * 判据：光的量与边缘的锐度。
 *
 * - `soft` 柔雾：光晕完全归零（与山形 soft 的三层光归零对齐），线加粗并略压暗，
 *   只保留形状；
 * - `balanced` 平衡：一层近场光晕（半径 9px），线宽不变；
 * - `neon` 锐光：线收细（×0.72）并提亮（×1.18），光晕更强更集中（半径 6.4px），
 *   每条线的线心可单独辨认。
 *
 * 只修改 uniform，不重建材质，切换档位不产生黑帧（与 `RidgeLayer.setGlow` 一致）。
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
  uniform float uTime;       // 秒：**水的时钟**。层间平移 / 包络 / 底噪都随它漂，暂停也不停
  uniform float uEnergy;     // 频谱能量（0–1）：只压频谱那部分高度，底噪不受它影响
  uniform float uNoise;      // 底噪幅度（占 uSpan 的比例）
  uniform float uHeightPx;   // 画布高度（CSS px）：uv 距离 → 像素距离
  uniform float uHorizon;    // 地平线的 uv.y
  uniform float uSpan;       // 水面占的 uv 高度
  uniform float uGain;       // 整层总增益（主题相关的观感微调）
  uniform float uLine;       // 线亮度系数（光效档位）
  uniform float uWidth;      // 线宽系数（>1 更软、<1 更锐）
  uniform float uHalo;       // 近层光晕强度
  uniform float uHaloK;      // 光晕半径（px）：越大收得越快

  // 一维值噪声（hash 加三次插值）。不使用多频 sin 叠加：sin 相加在固定视角下会
  // 呈现周期，水面读作来回摆动的条纹；值噪声无可辨周期，且仅需两次 hash 与三次
  // mix，在 22 层循环中开销可接受。
  float hash11(float p) {
    p = fract(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
  }

  float noise1(float x) {
    float i = floor(x);
    float f = fract(x);
    float u = f * f * (3.0 - 2.0 * f);
    return mix(hash11(i), hash11(i + 1.0), u) * 2.0 - 1.0;
  }

  vec3 lin2srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), c));
  }

  void main() {
    // 地平线以上不绘制。此判断省去一半像素 × 20 层的循环，收益高于任何片内
    // 优化——水面只位于下半屏。
    if (vUv.y > uHorizon + 0.02) discard;

    // 左右各 8% 与底部 6% 淡出。水面直接顶到窗口边缘会被读作图像被裁切，淡出后
    // 呈"水面延伸至画面之外"；底边同理——播放条覆盖其上，线被硬切会露出画布边界。
    float side = smoothstep(0.0, 0.08, vUv.x) * smoothstep(0.0, 0.08, 1.0 - vUv.x);
    side *= smoothstep(0.0, 0.06, vUv.y);
    if (side < 0.004) discard;

    float acc = 0.0;

    // 底噪的采样坐标在循环外算一次：横向波长由 vUv.x 定（粗 9 周期、细 23 周期
    // 每屏宽），相位由 uTime 推进。两个不整除的频率叠加，单一波长就看不出来。
    float nx1 = vUv.x * 9.0 + uTime * 0.26;
    float nx2 = vUv.x * 23.0 - uTime * 0.44;

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
      // 频谱部分的高度：**乘 uEnergy**，所以暂停（能量收平到 0）后这部分归零，
      // 只留下面的底噪。恢复播放时从 0 长起来，不接暂停前那一帧。
      float amp = uSpan * 0.34 * (0.15 + 0.85 * k) * envelope * uEnergy;

      // 底噪：水面自身的基础起伏，与 uEnergy 无关，真实频谱叠加其上。
      // 缺少此项时暂停画面为 22 条等高直线（频谱振幅为 0），会被判为渲染失败；
      // 保留后静止的只有频谱部分，水面仍在缓慢流动。
      // 层相位用 float(l) 乘不同系数：相位相同时 22 层会是同一条噪声曲线上下
      // 平移，读作格栅抖动；错开后与透视中的斜向波纹一致。
      float nAmp = uSpan * uNoise * (0.22 + 0.78 * k);
      float hNoise =
        (noise1(nx1 + float(l) * 2.3) * 0.62 + noise1(nx2 + float(l) * 5.1) * 0.38) * nAmp;

      // 两个方向的剔除：波形只向上鼓，上侧到波峰为止；下侧仅保留光晕半径。
      // 该判断使"像素不在此层附近"的情况省去一次纹理采样。
      // 上界需计入底噪，否则被噪声抬高的近层线会被提前剔除，形成虚线。
      float dy = (vUv.y - base) * uHeightPx;
      if (dy < -26.0 || dy > (amp + nAmp) * uHeightPx + 26.0) continue;

      // 带单位为 1、纹理方向为 1：平移量先除以 BANDS 再进 uv。
      // 每层 2.6 带、每秒 0.55 带：层间错位需足以看出斜向流动，但不能大到相邻层
      // 完全无关（那会退化为噪声）。漂移速度刻意放慢：背景需适合长时间观看，
      // 1%/s 的横向速度已可感知为动态。
      float off = (float(l) * 2.6 + uTime * 0.55) / ${BANDS}.0;
      float h = texture2D(uRamp, vec2(vUv.x + off, 0.5)).r;

      float d = (vUv.y - (base + h * amp + hNoise)) * uHeightPx;
      float w = (0.8 + 1.2 * k) * uWidth;    // 线宽 px：近处更粗；档位再乘一次
      float line = exp(-(d * d) / (w * w));  // 高斯截面，天然抗锯齿
      // 光晕只给近层：远层为 1px 细线，光晕在此既不可见又增加成本
      float halo = exp(-abs(d) / uHaloK) * uHalo * step(0.55, k);

      // 远层近乎不可见（0.05），近层实际成形，这是纵深的主要来源；均匀 alpha 会
      // 使所有线权重相同，读作格栅。上限取 0.35，因为背景之上有表格文字：线一旦
      // 比正文更醒目，背景即成为条纹贴图。
      acc += line * (0.05 + 0.30 * pow(k, 1.4)) * uLine + halo;
    }

    float a =
      clamp(acc * uGain * side, 0.0, 1.0) *
      // 地平线处淡出：最远层振幅最小，接近水平直线，直接出现会在地平线形成一道
      // 亮边，读作水面被横线封住。向上 5% uv 范围内渐隐，水面从暗处浮出且无边界。
      // 写作 1 - smoothstep(小, 大) 而非 smoothstep(大, 小)：GLSL 规定
      // edge0 >= edge1 时结果未定义，反向传参在部分驱动上返回随机值。
      (1.0 - smoothstep(uHorizon - 0.05, uHorizon, vUv.y));
    if (a < 0.002) discard;

    vec3 col = lin2srgb(uColor);
    // 预乘 + over：波纹虽属光，但叠在固定渐变之上时使用 over，避免亮色主题的
    // 浅底被加色至过曝（山形层同理，见 ridge.ts）。
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
  /** 暂停态：频谱收平（喂全 0 + gate），但水时钟照走，底噪仍流动 */
  private paused = false;
  /**
   * 收平/起振的过渡系数（0–1）。
   *
   * 暂停时直接跳 0 会"啪"一下没掉，像渲染挂了；这里用 70ms 时间常数做淡出，
   * 大约四帧收平——是"淡下去"，但仍然是一次性收干净，不停留旧形状。
   */
  private gate = 1;
  /**
   * 水的时钟：推进层间平移、包络相位与底噪演化。
   * **暂停也走**——它记的是"水面时间"，不是"音乐时间"。
   */
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
        // 底噪幅度（占 uSpan 的比例）。0.04 × 0.40 = 1.6% 屏高，800px 高时约
        // 13px 起伏：足以看出在动，又不会让安静段显得仍有能量。
        // 远层再乘 (0.22 + 0.78k)，近大远小随透视变化。
        uNoise: { value: 0.04 },
        uHeightPx: { value: 800 },
        // 地平线位于屏幕 66% 高度处：上半屏留给标题与列表，水面铺满下方约三分之一。
        // span 略超出底边（0.34 - 0.36 = -0.02），由 shader 的底部淡出处理，
        // 使最短的一层延伸至屏幕之外，近处不会突然停在地平线上方。
        uHorizon: { value: 0.40 },
        uSpan: { value: 0.40 },
        // 总增益：背景属氛围层，亮度不得超过列表文字。0.7 为可见但缩略图下几乎
        // 不被注意的量级（正在播放页的 2D 版为主视觉，不做同等折让）。
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
   * 播放与暂停。
   *
   * 暂停作用于频谱而非水面：
   * 1. 喂全 0 并收平 `gate`，使频谱部分高度归零，不保留暂停前的峰谷；
   * 2. 时钟继续推进，底噪涟漪仍在横向流动。
   *
   * 两种相反做法均不可行：仅冻结时间而不收平，画面会留下一片静止的起伏；仅收平
   * 而不提供底噪，则得到 22 条等高直线，读作渲染失败。
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

  /** 推进时钟与收平系数（时钟照走，只有频谱收平到 0） */
  private advance(dt: number): void {
    this.clock += dt;
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
