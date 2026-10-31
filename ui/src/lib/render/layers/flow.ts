/**
 * 背景流动渐变（docs/46 §3.1）。
 *
 * 作用：在固定 CSS 渐变之上叠一层缓慢漂移的彩色光雾，使背景保持动态而不
 * 干扰内容。不重绘配色方案——`--ambient-bg` 是配色资产的唯一来源，本层只
 * 增加动态，不修改配色资产。
 *
 * 画法为 5 个椭圆高斯斑叠加：exp(-r²) 处处可导，不存在阈值，因此不产生可辨
 * 边缘，也没有可辨识的形状，只有浓淡变化。基于噪声的做法（fbm + 域扭曲 +
 * smoothstep 遮罩）在阈值处一阶导不连续，暗背景上必然读得出边界，域扭曲还会
 * 把噪声坐标拉出折角。高斯叠加的采样成本亦更低（5 次 exp 对比 12 次噪声取样）。
 *
 * 两层结构（本文件的核心设计）：
 *
 *   低分辨率场（384×216，每 90ms 计算一次）
 *        ↓ 双线性放大 + 抖动
 *   全分辨率合成（每帧一次带纹理的四边形）
 *
 * 不直接在全屏计算的原因：场的成本与像素数成正比。4K@2x 为 8M 像素，而该场
 * 低频且变化极慢（时间系数 0.06，一轮约 100s），90ms 更新与逐帧更新在画面上
 * 无法区分——变化量小于 8bit 的一个色阶。昂贵的计算因此限定在 384×216 内，
 * 全分辨率仅做一次纹理采样。
 *
 * 8bit 目标配合极暗光雾必然产生色带，合成时叠加有序噪声（dither）可将其打散；
 * 代价是一次 hash 运算，不加则暗背景上会出现等高线状条纹。
 */

import { PlaneGeometry, Color, LinearFilter, Mesh, OrthographicCamera, Scene, ShaderMaterial, Vector2, WebGLRenderTarget, NoBlending, CustomBlending, OneFactor, OneMinusSrcAlphaFactor, AddEquation } from 'three';
import { onThemeChange, themeColor } from '../../accent';
import type { Frame, GlLayer, GlStage } from '../context';

/** 场的分辨率：低频内容在 384×216 已足够，更高分辨率无收益 */
const FIELD_W = 384;
const FIELD_H = 216;

/**
 * 场的最小更新间隔。
 *
 * 90ms 约合 11Hz。取值的依据是变化是否可辨：时间系数 0.06 下，90ms 内场的
 * 位移约 0.0054 个噪声周期，换算到 8bit 通道远小于 1 个色阶，逐帧更新与
 * 90ms 更新在画面上无差别。
 */
const FIELD_INTERVAL_MS = 90;

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const FRAG_FIELD = /* glsl */ `
  varying vec2 vUv;
  uniform float uTime;
  uniform float uAspect;
  uniform vec3 uColorA;
  uniform vec3 uColorB;
  uniform float uIntensity;

  /**
   * 单个椭圆高斯斑。
   *
   * exp(-r²) 在中心为 1、远处趋 0，处处可导。这使晕染与基于阈值的形状区分开来：
   * fbm 加 smoothstep 由阈值切出形状，阈值处存在一阶或二阶不连续，因而出现边缘
   * 与色块；高斯斑没有可辨形状，只有浓淡变化。
   */
  float blob(vec2 p, vec2 c, vec2 r) {
    vec2 d = (p - c) / r;
    return exp(-dot(d, d) * 1.35);
  }

  /** 线性工作色 → sRGB 编码。画布被浏览器当作 sRGB 解释，不编码就会整体偏暗 */
  vec3 lin2srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(lo, hi, step(vec3(0.0031308), c));
  }

  void main() {
    // uv 直接用 0–1：位置写"屏幕的百分之几"比写"几个噪声周期"好理解，
    // 也不随窗口比例漂移。半径的 x 分量除 uAspect，否则宽屏上斑会被拉成横条。
    vec2 p = vUv;
    float ar = max(uAspect, 1.0);
    float t = uTime;

    // 五个斑。位置在 ±0.05 视口内缓慢摆动（周期约 90–150s），
    // 半径刻意给得大（0.3–0.45 视口）让相邻斑**始终重叠**——
    // 一旦两个斑分开，边缘就会读出"两个圆"，那又变回形状了。
    float s = 0.0;
    s += 1.00 * blob(p, vec2(0.26 + 0.06 * sin(t * 0.90), 0.84 + 0.05 * cos(t * 0.70)), vec2(0.40 / ar, 0.34));
    s += 0.86 * blob(p, vec2(0.74 + 0.05 * cos(t * 0.80), 0.70 + 0.06 * sin(t * 1.05)), vec2(0.38 / ar, 0.32));
    s += 0.62 * blob(p, vec2(0.48 + 0.05 * sin(t * 0.62 + 1.7), 1.04 + 0.04 * cos(t * 0.52)), vec2(0.48 / ar, 0.38));
    s += 0.50 * blob(p, vec2(0.92 + 0.05 * sin(t * 0.71 + 2.4), 0.28 + 0.05 * cos(t * 0.88)), vec2(0.30 / ar, 0.26));
    s += 0.44 * blob(p, vec2(0.08 + 0.04 * cos(t * 0.54 + 3.1), 0.42 + 0.04 * sin(t * 0.78)), vec2(0.28 / ar, 0.24));

    // 软饱和：重叠处不至于叠爆，且仍然是连续函数（1-exp 没有拐点）。
    // 用 clamp 会在超限处压出一条"等值线"，暗背景上能看见。
    float a = 1.0 - exp(-s * 1.05);

    // 混色跟着浓淡走。混色在线性空间做（THREE.Color 存的就是线性值），
    // 否则两色过渡中段会比两端更灰。
    vec3 col = lin2srgb(mix(uColorA, uColorB, clamp(s * 0.75, 0.0, 1.0)));
    float alpha = a * uIntensity;
    // 预乘输出：画布按 premultipliedAlpha 合成，rgb 必须是 col*alpha
    gl_FragColor = vec4(col * alpha, alpha);
  }
`;

const FRAG_COMPOSITE = /* glsl */ `
  varying vec2 vUv;
  uniform sampler2D uField;

  float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
  }

  void main() {
    vec4 c = texture2D(uField, vUv);
    // 抖动：打散 8bit 色阶在暗背景上的等高线（±0.5 个色阶，肉眼不可辨）
    float d = hash(gl_FragCoord.xy) - 0.5;
    c.rgb += d / 255.0;
    gl_FragColor = c;
  }
`;

let quad: PlaneGeometry | null = null;
function sharedQuad(): PlaneGeometry {
  if (!quad) quad = new PlaneGeometry(2, 2);
  return quad;
}

export class FlowLayer implements GlLayer {
  readonly name = 'flow';

  private readonly fieldScene = new Scene();
  private readonly compositeScene = new Scene();
  private readonly camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly target: WebGLRenderTarget;
  private readonly fieldMaterial: ShaderMaterial;
  private readonly compositeMaterial: ShaderMaterial;
  private readonly unsubscribeTheme: () => void;

  private lastFieldAt = -Infinity;
  private fieldDirty = true;
  private light = false;

  constructor() {
    this.target = new WebGLRenderTarget(FIELD_W, FIELD_H, {
      // 线性过滤：放大时的双线性插值本身就是"免费柔化"
      minFilter: LinearFilter,
      magFilter: LinearFilter,
      depthBuffer: false,
      stencilBuffer: false,
    });

    this.fieldMaterial = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG_FIELD,
      uniforms: {
        uTime: { value: 0 },
        uAspect: { value: FIELD_W / FIELD_H },
        uColorA: { value: new Color('#7c9cff') },
        uColorB: { value: new Color('#9ec6ff') },
        uIntensity: { value: 0.2 },
      },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      // 写目标是"存值"不是"混合"：开启混合会让上一次的内容渗进这一次
      blending: NoBlending,
    });

    this.compositeMaterial = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: FRAG_COMPOSITE,
      uniforms: { uField: { value: this.target.texture } },
      transparent: true,
      depthTest: false,
      depthWrite: false,
      // 需使用 CustomBlending 显式指定因子，不能用 `AdditiveBlending`：
      // three 的 AdditiveBlending 为 (SRC_ALPHA, ONE)，而本着色器输出已是预乘色
      // (col*alpha, alpha)，再乘一次 alpha 等于取平方——实测云团最亮处 rgb 仅剩
      // 1/255，画面接近全黑（仅 alpha 在累加）。预乘加色应为 (ONE, ONE)，
      // 见下方 syncTheme。
      blending: CustomBlending,
      blendSrc: OneFactor,
      blendDst: OneFactor,
      blendEquation: AddEquation,
    });

    const mesh = new Mesh(sharedQuad(), this.fieldMaterial);
    mesh.frustumCulled = false;
    this.fieldScene.add(mesh);

    const composite = new Mesh(sharedQuad(), this.compositeMaterial);
    composite.frustumCulled = false;
    this.compositeScene.add(composite);

    this.syncTheme();
    this.unsubscribeTheme = onThemeChange(() => this.syncTheme());
  }

  /**
   * 颜色跟随 `--accent`，副色由主色偏移色相得到 —— 不给 5 套方案各写一组
   * 硬编码颜色：那样加第 6 套方案时这里会静默用错色，而且没人会发现。
   */
  private syncTheme(): void {
    const light =
      typeof document !== 'undefined' &&
      document.documentElement.getAttribute('data-theme') === 'light';
    this.light = light;

    const accent = new Color(themeColor('--accent'));
    const secondary = accent.clone().offsetHSL(0.09, 0.02, 0.08);
    this.fieldMaterial.uniforms.uColorA!.value = accent;
    this.fieldMaterial.uniforms.uColorB!.value = secondary;
    // 浅色主题下加法混合会洗成一片白：改用"正常混合"的变暗着色
    this.fieldMaterial.uniforms.uIntensity!.value = light ? 0.075 : 0.12;

    this.compositeMaterial.blending = CustomBlending;
    // 预乘 alpha 的两种合成，因子都从 ONE 起：
    // - 暗色主题用加色（ONE, ONE）—— 光雾"发光"叠在渐变上
    // - 浅色主题用 over（ONE, ONE_MINUS_SRC_ALPHA）—— 加色会把浅底洗白成一片
    this.compositeMaterial.blendSrc = OneFactor;
    this.compositeMaterial.blendDst = light ? OneMinusSrcAlphaFactor : OneFactor;
    this.compositeMaterial.blendEquation = AddEquation;
    this.fieldDirty = true;
  }

  resize(width: number, height: number): void {
    // 场是正方形像素的 render target，宽高比只用来把光斑在屏幕上还原成椭圆
    this.fieldMaterial.uniforms.uAspect!.value = height > 0 ? width / height : FIELD_W / FIELD_H;
  }

  draw(stage: GlStage, frame: Frame): void {
    const nowMs = frame.time * 1000;
    if (this.fieldDirty || nowMs - this.lastFieldAt >= FIELD_INTERVAL_MS) {
      this.lastFieldAt = nowMs;
      this.fieldDirty = false;
      // 时间系数 0.06（docs/46 §3.1）：整场跑一轮约 100s，是"慢速"的量级
      this.fieldMaterial.uniforms.uTime!.value = frame.time * 0.06;

      stage.renderer.setRenderTarget(this.target);
      stage.renderer.render(this.fieldScene, this.camera);
      stage.renderer.setRenderTarget(null);
    }

    stage.renderer.render(this.compositeScene, this.camera);
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.target.dispose();
    this.fieldMaterial.dispose();
    this.compositeMaterial.dispose();
    // 共享几何体不销毁：其他图层还在用（由模块级生命周期管）
  }
}

/** 供调试面板/测试读的常量（不参与渲染） */
export const FLOW_FIELD_SIZE = new Vector2(FIELD_W, FIELD_H);
