/**
 * 山形频谱图层：以 WebGL 绘制背景山形并叠加边缘光晕。
 *
 * 不对 Canvas 2D 使用 `shadowBlur`：它是逐像素高斯，在 160×90 的 buffer 上
 * 尚可接受，全分辨率下开销过高。该形状的光晕无需卷积——每个像素到脊线的
 * 垂直距离在 fragment 中即可获得，光晕即为该距离的指数衰减。因此只需一次
 * pass，无离屏缓冲与模糊核，并可同时输出描边、内层光与外层弥散三种半径。
 *
 * 数值规则与 `ridge-curve.ts` 保持一致：
 * - 时间平滑 0.3/帧、`v^1.2` 伽马、近山 0.82 全幅、远山倒序 0.5 幅
 * - 竖直渐变填充（顶部实、底边透明），此处用 `grad = vUv.y` 复现
 * - 与 Canvas 2D 实现的差异仅在于光栅器：折线与中点二次贝塞尔改为
 *   Catmull-Rom 256 点，blur 改为解析光晕
 */

import { DataUtils, DataTexture, HalfFloatType, LinearFilter, Mesh, OrthographicCamera, PlaneGeometry, RedFormat, Scene, ShaderMaterial, Color, CustomBlending, OneFactor, OneMinusSrcAlphaFactor, AddEquation } from 'three';
import { onThemeChange, themeColor } from '../../accent';
import type { RidgeGlow } from '../../settings';
import type { SpectrumSource } from '../../spectrum-source';
import type { Frame, GlLayer, GlStage } from '../context';
import { RidgeCurve } from '../ridge-curve';

/** 重采样点数：256 点 @ 2560px 宽 = 每段 10px，配合线性过滤已看不出折角 */
const POINTS = 256;
/** 48 带由数据源固定给出（`RemoteSpectrumSource` 的 BANDS） */
const BANDS = 48;

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const FRAG = /* glsl */ `
  varying vec2 vUv;
  uniform sampler2D uRamp;      // 256×2：row0 = 近山高度，row1 = 远山高度（0–1）
  uniform vec3 uColor;
  uniform float uBodyNear;
  uniform float uBodyFar;
  uniform float uRim;           // 描边强度
  uniform float uRimK;          // 描边半径（1/px）
  uniform float uGlow;
  uniform float uGlowK;
  uniform float uHalo;
  uniform float uHaloK;
  uniform float uHeightPx;      // 画布高度（CSS px）：把 uv 距离换算成像素距离

  vec3 lin2srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), c));
  }

  /** 一条山脊的贡献：填充 + 三层同心光。d 是像素距离，>0 在山体内部 */
  float ridge(float d, float body) {
    // 填充的抗锯齿边只有 ~1.5px，不要再宽——宽了会变成"糊边"，脊线就没了
    float inside = smoothstep(0.0, 1.5, d);
    float a = inside * body * vUv.y;   // 竖直渐变：顶部实、底边透明
    a += uRim * exp(-abs(d) * uRimK);
    a += uGlow * exp(-abs(d) * uGlowK);
    a += uHalo * exp(-abs(d) * uHaloK);
    return a;
  }

  void main() {
    // v=0.25 / 0.75 分别落在两行中心（线性过滤不会跨行混色）
    float hN = texture2D(uRamp, vec2(vUv.x, 0.25)).r;
    float hF = texture2D(uRamp, vec2(vUv.x, 0.75)).r;

    // d > 0 必须对应脊线下方（山体自底边升起，Canvas 2D 实现为
    // moveTo(0,H) → 曲线 → lineTo(W,H)）。写成 vUv.y - h 会把山体填到脊线
    // 以上，上半屏被实色覆盖，表现为背景整体过亮。
    // 注意：本段位于模板字符串内，注释中不可使用反引号
    float dN = (hN - vUv.y) * uHeightPx;
    float dF = (hF - vUv.y) * uHeightPx;

    float aNear = ridge(dN, uBodyNear);
    // 远山整体压到 0.55：它只是纵深，不该和近山抢注意力
    float aFar = ridge(dF, uBodyFar) * 0.55;

    float a = clamp(aNear + aFar, 0.0, 1.0);
    if (a < 0.002) discard;

    vec3 col = lin2srgb(uColor);
    // 预乘输出 + over 合成（山是"实形"，不是发光体：不能当加色画）
    gl_FragColor = vec4(col * a, a);
  }
`;

interface GlowPreset {
  bodyNear: number;
  bodyFar: number;
  rim: number;
  rimK: number;
  glow: number;
  glowK: number;
  halo: number;
  haloK: number;
}

/**
 * 三档光晕。单位为像素，`K` 越大衰减越快。
 *
 * - `soft` 纯填充，三层光晕全部归零，作为无光参照
 * - `balanced` 取两者中点（rim 0.03 / glow 0.025 / halo 0.015）：脊线处亮度
 *   增加约 7%，衰减范围仅 1–3px，整屏差异不可测
 * - `neon` 强档（脊线清晰、近场光明显），对应偏好锐利观感的情形
 */
const PRESETS: Record<RidgeGlow, GlowPreset> = {
  soft: { bodyNear: 0.30, bodyFar: 0.24, rim: 0, rimK: 0.30, glow: 0, glowK: 0.030, halo: 0, haloK: 0.006 },
  balanced: { bodyNear: 0.30, bodyFar: 0.24, rim: 0.03, rimK: 0.55, glow: 0.025, glowK: 0.055, halo: 0.015, haloK: 0.010 },
  neon: { bodyNear: 0.28, bodyFar: 0.22, rim: 0.28, rimK: 0.9, glow: 0.18, glowK: 0.095, halo: 0.08, haloK: 0.014 },
};

let quad: PlaneGeometry | null = null;
function sharedQuad(): PlaneGeometry {
  if (!quad) quad = new PlaneGeometry(2, 2);
  return quad;
}

export class RidgeLayer implements GlLayer {
  readonly name = 'ridge';

  private readonly scene = new Scene();
  private readonly camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly material: ShaderMaterial;
  private readonly texture: DataTexture;
  /**
   * 256×2 的半精度浮点纹理：R 通道存高度。
   *
   * 不能使用 8bit：量化到 1/255 在 800px 高的画布上形成 3px 台阶，台阶侧壁
   * 竖直且仅有 1.5px 抗锯齿，低频带剧烈起伏时会排列出垂直色带。half-float
   * 的 10 位尾数足以平滑，且 R16F 在 WebGL2 中可原生线性过滤（R32F 需要
   * `OES_texture_float_linear` 扩展）。
   */
  private readonly heights = new Uint16Array(POINTS * 2);
  private readonly curve = new RidgeCurve({ bandCount: BANDS, points: POINTS });
  private readonly unsubscribeTheme: () => void;
  /** 暂停时喂给曲线的一帧全 0：让曲线自己衰减回底边，不留残峰 */
  private readonly zero = new Float32Array(BANDS);
  private paused = false;
  /**
   * 收平系数。暂停时 70ms 淡到 0（约四帧），不是硬切——硬切看着像渲染挂了。
   * 停播时山形要"落回底边"，而不是凝固在暂停前那一帧的高度上。
   */
  private gate = 1;

  constructor(
    private readonly source: SpectrumSource,
    /** 初始档位；之后由 `setGlow` 驱动（组件里用单独的 effect，避免换档重建图层） */
    glow: RidgeGlow = 'balanced',
  ) {
    this.texture = new DataTexture(this.heights, POINTS, 2, RedFormat, HalfFloatType);
    this.texture.minFilter = LinearFilter;
    this.texture.magFilter = LinearFilter;
    this.texture.generateMipmaps = false;
    // 高度是"可插值的标量"，不是颜色：不要把 sRGB 解码套上去
    this.texture.needsUpdate = true;

    this.material = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG,
      uniforms: {
        uRamp: { value: this.texture },
        uColor: { value: new Color('#7c9cff') },
        uBodyNear: { value: 0.46 },
        uBodyFar: { value: 0.36 },
        uRim: { value: 0.34 },
        uRimK: { value: 0.55 },
        uGlow: { value: 0.32 },
        uGlowK: { value: 0.055 },
        uHalo: { value: 0.26 },
        uHaloK: { value: 0.01 },
        uHeightPx: { value: 800 },
      },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      // 预乘 alpha 的 over：山是实形，遮住下面的流动渐变
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

  /** 换档位只改 uniform，**不重建上下文/材质**（否则切档会闪一帧黑） */
  setGlow(glow: RidgeGlow): void {
    const p = PRESETS[glow] ?? PRESETS.balanced;
    const u = this.material.uniforms;
    u.uBodyNear!.value = p.bodyNear;
    u.uBodyFar!.value = p.bodyFar;
    u.uRim!.value = p.rim;
    u.uRimK!.value = p.rimK;
    u.uGlow!.value = p.glow;
    u.uGlowK!.value = p.glowK;
    u.uHalo!.value = p.halo;
    u.uHaloK!.value = p.haloK;
  }

  private syncTheme(): void {
    this.material.uniforms.uColor!.value = new Color(themeColor('--accent'));
  }

  resize(_width: number, height: number): void {
    this.material.uniforms.uHeightPx!.value = height;
  }

  draw(stage: GlStage, frame: Frame): void {
    // 收平系数（暂停 → 0）
    const target = this.paused ? 0 : 1;
    const k = 1 - Math.exp(-frame.dt / 0.07);
    this.gate += (target - this.gate) * k;
    if (Math.abs(this.gate - target) < 0.002) this.gate = target;

    // 暂停时喂全 0：曲线衰减到底边，恢复播放时从底边重新长起来
    this.curve.update(this.paused ? this.zero : this.source.frame(), frame.dt);
    this.upload();
    stage.renderer.render(this.scene, this.camera);
  }

  /**
   * 播放与暂停：暂停时山形落回底边，不停留在暂停瞬间的起伏上。
   * 与 `RippleLayer.setPaused` 语义一致。
   */
  setPaused(paused: boolean): void {
    this.paused = paused;
  }

  /**
   * 高度 → half-float 纹理。
   *
   * 选 R16F 而非 R32F：R32F 在 WebGL2 中不可线性过滤（需
   * `OES_texture_float_linear`），而脊线必须依赖线性过滤才不会呈现 256 段折线；
   * R16F 为 core 保证可过滤的格式。10 位尾数在 0–1 高度上的步进约 0.001
   * （800px 上约 0.8px），量化误差不可见。
   *
   * `DataUtils.toHalfFloat` 需 256×2 次调用，每帧成本低于 0.05ms。
   */
  private upload(): void {
    const { near, far } = this.curve;
    const h = this.heights;
    const gate = this.gate;
    for (let i = 0; i < POINTS; i += 1) {
      h[i] = DataUtils.toHalfFloat((near[i] as number) * gate);
      h[POINTS + i] = DataUtils.toHalfFloat((far[i] as number) * gate);
    }
    this.texture.needsUpdate = true;
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.texture.dispose();
    this.material.dispose();
  }
}
