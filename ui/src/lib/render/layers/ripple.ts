/**
 * 背景波纹图层：深度切片的相干波场。
 *
 * 画法：把下半屏读作一片朝观者铺开的水面，22 层是**同一张高度场**沿视线方向
 * 的连续截面。第 l 层的线为
 *
 *     y = base(k) + noise(x, k·FREQ_Z − t·DRIFT) · env(x) · persp(k)
 *
 * 三个因子各管一件事：
 * - noise：横向 × 深度的连续噪声场。相邻层的采样坐标只差 FREQ_Z/(层数−1)，
 *   因此相邻两条线天然相似——层间"相干"由此而来。坡面朝向观者处相邻层收拢
 *   （线密度升高，读作明暗），背向处摊开，布料般的疏密起伏即是这份相干性；
 *   噪声场随 uTime 沿深度轴漂移，波包从地平线缓缓走向近前。层间相关是这套
 *   画法的命门：若每层独立取形状（横向平移、独立相位），相邻层互不相关，
 *   叠加读作交织的乱线。
 * - env(x)：全层共享的频谱包络。48 带经 RidgeCurve 平滑重采样的曲线（与山形
 *   共用同一套数值规则）在这里不作为波的形状，而是作为波的**幅度轴**：压缩
 *   动态范围后与全幅能量混合——整片水面随音乐呼吸，能量所在的频段再额外
 *   隆起。包络再乘横向斜面（左 1.0 → 右 0.35 的直线缓降）：水面高左低右，
 *   读作朝右放平的斜坡；斜面只改幅度，层的基线依旧水平。音乐由此进入
 *   画面——形状连续演化，不因逐带跳动而破碎。
 * - persp(k)：近层宽、亮、疏，远层窄、暗、密；**振幅沿深度近乎不变**——壁纸
 *   的几何是每片切片采样同一场，纵深由亮度与层间距表达，压扁远层振幅会失去
 *   布料质感。
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
 * 光效档位（与山形共用同一份设置 `ridgeGlow`）。
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
  uniform sampler2D uRamp;   // 256×1：横向频谱包络（0–1），全层共享
  uniform vec3 uColor;
  uniform float uTime;       // 秒：**水的时钟**。深度漂移随它走，暂停也不停
  uniform float uEnergy;     // 频谱能量（0–1）：只压频谱那部分包络，底浪不受它影响
  uniform float uNoise;      // 静息幅度（占 uSpan 的比例）：能量归零后的底浪
  uniform float uCurveAmp;   // 频谱包络增益：能量满格时在底浪之上再加的幅度
  uniform float uHeightPx;   // 画布高度（CSS px）：uv 距离 → 像素距离
  uniform float uHorizon;    // 地平线的 uv.y
  uniform float uSpan;       // 水面占的 uv 高度
  uniform float uGain;       // 整层总增益（主题相关的观感微调）
  uniform float uLine;       // 线亮度系数（光效档位）
  uniform float uWidth;      // 线宽系数（>1 更软、<1 更锐）
  uniform float uHalo;       // 近层光晕强度
  uniform float uHaloK;      // 光晕半径（px）：越大收得越快

  // 横向噪声频率：全宽约 2.0 个基频鼓包（与壁纸一致的大包尺度），二、三倍频
  // 补充细节。过密会碎，过疏则水面近于几条独立弧线
  const float FREQ_X = 2.0;
  // 深度向频率：22 层横跨 0.65 个噪声单元，相邻层只差 0.031——层间相干靠它
  // 量化保证；跨度过大（>1）时首末层完全无关，相干性消失
  const float FREQ_Z = 0.65;
  // 横向斜切：深度坐标随 x 偏移，波峰的连线在屏幕上呈对角走向（透视中波面
  // 斜向观者的观感）。没有它时各层波峰垂直对齐，画面读作竖直条纹
  const float SHEAR = 0.35;
  // 深度漂移速度（噪声单元/秒）：一个波包从地平线走到近前约 19 秒。负号使
  // 采样点随时间移向更小的深度坐标，波包表现为向观者缓缓流来而非远去；
  // 速度上限由"流动感"决定——过快时运动读作波浪推进，而非水面流动
  const float DRIFT = 0.035;

  // 二维值噪声（hash 加双三次平滑插值）。不使用多频 sin 叠加：sin 相加在固定
  // 视角下会呈现周期，水面读作来回摆动的条纹；值噪声无可辨周期，开销在 22 层
  // 循环中可接受。
  float hash21(vec2 p) {
    vec3 q = fract(vec3(p.xyx) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
  }

  float noise2(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + vec2(1.0, 0.0));
    float c = hash21(i + vec2(0.0, 1.0));
    float d = hash21(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
  }

  // 三倍频 fbm：倍频间带旋转与微差步长（2.02/2.03），避免八度对齐出周期
  const mat2 ROT = mat2(0.8, 0.6, -0.6, 0.8);
  float fbm(vec2 p) {
    float v = 0.5 * noise2(p);
    p = ROT * p * 2.02;
    v += 0.25 * noise2(p);
    p = ROT * p * 2.03;
    v += 0.125 * noise2(p);
    return v / 0.875;
  }

  vec3 lin2srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), c));
  }

  void main() {
    // 地平线以上不绘制。此判断省去一半像素 × 22 层的循环，收益高于任何片内
    // 优化——水面只位于下半屏。
    if (vUv.y > uHorizon + 0.02) discard;

    // 左右各 8% 与底部 6% 淡出。水面直接顶到窗口边缘会被读作图像被裁切，淡出后
    // 呈"水面延伸至画面之外"；底边同理——播放条覆盖其上，线被硬切会露出画布边界。
    float side = smoothstep(0.0, 0.08, vUv.x) * smoothstep(0.0, 0.08, 1.0 - vUv.x);
    side *= smoothstep(0.0, 0.06, vUv.y);
    if (side < 0.004) discard;

    // 频谱包络在循环外算一次：它只依赖 x，全层共享。曲线不能直接作包络：
    // 频谱能量天然向低频聚拢（高频带值常在低频的一成以下），直接乘会令高频侧
    // 的水面只剩静息底浪。先 pow 0.6 压缩动态范围，再与全幅能量按 0.55 混合
    // ——整片水面随音乐呼吸，能量所在的频段在其上额外隆起。
    float curve = texture2D(uRamp, vec2(vUv.x, 0.5)).r;
    float shape = pow(curve, 0.6);
    // 横向斜面：振幅自左向右沿直线缓降（1.0 → 0.35），整片水面读作一面朝右
    // 放平的斜坡。只乘在幅度上——层底与线依旧水平，不是把画面整体倾斜；
    // 右侧保有约三分之一幅度，是"低"而不是"停"
    float ramp = mix(1.0, 0.35, vUv.x);
    float env = (uNoise + uCurveAmp * uEnergy * mix(1.0, shape, 0.55)) * ramp;

    float acc = 0.0;

    // 远（k=0，靠近地平线）→ 近（k=1，屏幕底）：远层窄、淡、密，近层宽、亮、疏。
    // 指数 1.55 是 2D 版实测出来的透视曲线：线性会让远处的层间距同样大，
    // 看着像等距横条而不是纵深。
    for (int l = 0; l < ${LAYERS}; l++) {
      float k = float(l) / float(${LAYERS - 1});
      float base = uHorizon - uSpan * pow(k, 1.55);

      // 幅度沿深度近乎不变：壁纸的几何是每一片都采样同一场，切片之间只有
      // 亮度与间距不同；若近大远小，远层会被压扁成雾状横纹，布料质感消失。
      // 只保留轻微的近大远小（0.75 + 0.25k）维持水面透视的暗示
      float amp = env * uSpan * (0.75 + 0.25 * k);

      // 该像素到此层底的竖直距离（像素）。上下剔除必须先于 fbm：远层波高小，
      // 下半屏大多数像素会被剔掉，fbm（12 次 hash）是本循环最贵的一步
      float dy = (vUv.y - base) * uHeightPx;
      if (dy < -26.0 || dy > amp * uHeightPx + 26.0) continue;

      // 噪声场采样坐标：横向由像素 x 定，深度由层号 k 定并随时间漂移。
      // 相邻层的坐标差只有 FREQ_Z/(层数-1)，噪声值几乎相同——相邻线因此
      // 相似而不相同，疏密沿坡向收拢/摊开。
      vec2 np = vec2(vUv.x * FREQ_X, vUv.x * SHEAR + k * FREQ_Z - uTime * DRIFT);
      // 单侧化：fbm ∈ 约[-1,1]，映射到 [0,1] 后波只从层底向上鼓，符合
      // "水面自地平线铺向下半屏"的既定视觉语言。再取平方：质量压向谷底、
      // 峰更立，水面呈缓谷尖峰，谷底处相邻层收拢到层底附近
      float s = 0.5 + 0.5 * fbm(np);
      float n01 = s * s;
      float h = dy - n01 * amp * uHeightPx;

      float w = (0.8 + 1.2 * k) * uWidth;    // 线宽 px：近处更粗；档位再乘一次
      float line = exp(-(h * h) / (w * w));  // 高斯截面，天然抗锯齿
      // 光晕只给近层：远层为 1px 细线，光晕在此既不可见又增加成本
      float halo = exp(-abs(h) / uHaloK) * uHalo * step(0.55, k);

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
  /** 暂停时喂给曲线的一帧全 0：让曲线自己衰减回静息值，不留残峰 */
  private readonly zero = new Float32Array(BANDS);
  /** 暂停态：频谱收平（喂全 0 + gate），但水时钟照走，底浪仍流动 */
  private paused = false;
  /**
   * 收平/起振的过渡系数（0–1）。
   *
   * 暂停时直接跳 0 会"啪"一下没掉，像渲染挂了；这里用 70ms 时间常数做淡出，
   * 大约四帧收平——是"淡下去"，但仍然是一次性收干净，不停留旧形状。
   */
  private gate = 1;
  /**
   * 水的时钟：推进噪声场的深度漂移。
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
    // 采样点固定在 vUv.x（无层间平移、无绕回），默认 ClampToEdge 即可，
    // 不需要 RepeatWrapping——曲线不是周期函数，绕回采样会在屏中出现接缝
    this.texture.needsUpdate = true;

    this.material = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      uniforms: {
        uRamp: { value: this.texture },
        uColor: { value: new Color('#7c9cff') },
        uTime: { value: 0 },
        uEnergy: { value: 0 },
        // 基础波幅度：0.36 × 0.40(uSpan) ≈ 14% 屏高，是这个形态的**主体**
        // 而非安静段的保底——大幅布料起伏常驻，与壁纸的观感同级
        uNoise: { value: 0.36 },
        // 频谱包络增益：满能量时近层波高约 (0.30+0.55)×0.40 = 34% 屏高，
        // 恰在水面带（40% 屏高）之内；音乐把基础波再抬高一半
        uCurveAmp: { value: 0.55 },
        uHeightPx: { value: 800 },
        // 地平线位于屏幕 40% 高度处：上半屏留给标题与列表，水面铺满下方约三分之一。span 略超出底边（0.34 - 0.36 = -0.02），由
        // shader 的底部淡出处理，使最短的一层延伸至屏幕之外，近处不会突然停在地平线上方。
        uHorizon: { value: 0.40 },
        uSpan: { value: 0.40 },
        // 总增益：背景属氛围层，亮度不得超过列表文字。0.72 为可见但缩略图下几乎
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
   * 暂停作用于包络而非水面：
   * 1. 喂全 0 并收平 `gate`，频谱包络项归零，不保留暂停前的隆起；
   * 2. 时钟继续推进，噪声场仍沿深度漂移，底浪照常流动。
   *
   * 两种相反做法均不可行：仅冻结时间而不收平，画面会留下一片静止的隆起；仅收平
   * 而不保留底浪，则得到 22 条等距直线，读作渲染失败。
   */
  setPaused(paused: boolean): void {
    this.paused = paused;
  }

  private syncTheme(): void {
    this.material.uniforms.uColor!.value = new Color(themeColor('--accent'));
  }

  resize(_width: number, height: number): void {
    this.material.uniforms.uHeightPx!.value = height;
  }

  draw(stage: GlStage, frame: Frame): void {
    this.advance(frame.dt);
    // 暂停时喂全 0：曲线自身的平滑会把残余隆起衰减掉，配合 gate 收平，
    // 恢复播放时从底浪重新长起来，而不是"接上暂停前那一帧"
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
   * 不乘环形窗、不乘 gate：采样点固定在 vUv.x，两端无需压平；收平由 uEnergy
   * 一处承担（纹理保持曲线原值，暂停时其贡献本来就被能量压到 0）。
   * 能量按曲线均值归一：均值 0.32 视为"满浪"（与 2D 版同一口径）。
   */
  private upload(): void {
    const near = this.curve.near;
    const h = this.heights;
    let energySum = 0;
    for (let i = 0; i < POINTS; i += 1) {
      const v = near[i] as number;
      energySum += v;
      h[i] = DataUtils.toHalfFloat(v);
    }
    this.material.uniforms.uEnergy!.value = Math.min(1, energySum / POINTS / 0.32) * this.gate;
    this.texture.needsUpdate = true;
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.texture.dispose();
    this.material.dispose();
  }
}
