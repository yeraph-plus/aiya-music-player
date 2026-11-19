import { AddEquation, Color, CustomBlending, DataTexture, DataUtils, HalfFloatType, LinearFilter, Mesh, OneFactor, OneMinusSrcAlphaFactor, OrthographicCamera, PlaneGeometry, RedFormat, Scene, ShaderMaterial } from 'three';
import { onThemeChange, themeColor } from '../../accent';
import type { SpectrumSource } from '../../spectrum-source';
import type { Frame, GlLayer, GlStage } from '../context';
import { SoftSpectrum } from '../soft-spectrum';

type Shape = 'aurora' | 'curtain';
const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() { vUv = uv; gl_Position = vec4(position.xy, 0., 1.); }
`;
const COMMON = /* glsl */ `
  varying vec2 vUv;
  uniform float uTime, uGate, uLight;
  uniform vec3 uA, uB;
  uniform sampler2D uSoft;
  float band(float x) { return texture2D(uSoft, vec2(clamp(x, 0., 1.), .5)).r; }
  vec3 srgb(vec3 c) {
    return mix(c * 12.92, 1.055 * pow(max(c, vec3(0.)), vec3(1./2.4)) - .055,
      step(vec3(.0031308), c));
  }
  vec3 color(float x) {
    vec3 c = mix(uA, uB, clamp(x, 0., 1.));
    return srgb(mix(c, c * .55, uLight));
  }
`;
const FRAGMENTS: Record<Shape, string> = {
  aurora: COMMON + /* glsl */ `
    void main() {
      vec3 c = vec3(0.); float alpha = 0.;
      for (int i = 0; i < 5; i++) {
        float fi = float(i), x = vUv.x;
        float fold = sin(x*5. + uTime*.12 + fi*.52)*.055
          + sin(x*9. - uTime*.09 + fi)*.024;
        float y = .13 + fi*.052 + fold + band(x*.8)*.13;
        float d = vUv.y - y;
        float veil = exp(-pow(d/(.045 + fi*.01), 2.))*.15;
        float edge = exp(-abs(d)/.003)*.045;
        float a = (veil + edge)*(.30 + .70*sin(x*3.14159))*uGate;
        c += color(fi/4.)*a; alpha += a;
      }
      gl_FragColor = vec4(c, alpha);
    }
  `,
  curtain: COMMON + /* glsl */ `
    void main() {
      vec3 c = vec3(0.); float alpha = 0.;
      for (int i = 0; i < 12; i++) {
        float fi = float(i), x = (fi+.5)/12.;
        float center = x + sin(uTime*.08 + fi*1.1)*.045;
        float e = band(x), h = .16 + e*.43;
        float d = (vUv.x-center)/(.028 + e*.025);
        float vertical = exp(-pow((vUv.y-.1)/h, 2.));
        float a = exp(-d*d)*vertical*(.07+.08*e)*uGate;
        c += color(x)*a; alpha += a;
      }
      gl_FragColor = vec4(c, alpha);
    }
  `,
};

/** 两种柔光背景共用采样与合成，形态由各自的 fragment 实现。 */
export class AtmosphereLayer implements GlLayer {
  readonly name: Shape;
  private readonly scene = new Scene();
  private readonly camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly geometry = new PlaneGeometry(2, 2);
  private readonly curve = new SoftSpectrum();
  private readonly samples = new Uint16Array(256);
  private readonly zero = new Float32Array(48);
  private readonly texture: DataTexture;
  private readonly material: ShaderMaterial;
  private readonly unsubscribeTheme: () => void;
  private time = 0;
  private gate = 0;
  private paused = false;

  constructor(private readonly source: SpectrumSource, shape: Shape) {
    this.name = shape;
    this.texture = new DataTexture(this.samples, 256, 1, RedFormat, HalfFloatType);
    this.texture.minFilter = this.texture.magFilter = LinearFilter;
    this.texture.generateMipmaps = false;
    this.texture.needsUpdate = true;
    this.material = new ShaderMaterial({
      vertexShader: VERT, fragmentShader: FRAGMENTS[shape],
      uniforms: {
        uTime: { value: 0 }, uGate: { value: 0 }, uLight: { value: 0 },
        uA: { value: new Color() }, uB: { value: new Color() }, uSoft: { value: this.texture },
      },
      transparent: true, depthTest: false, depthWrite: false,
      blending: CustomBlending, blendSrc: OneFactor,
      blendDst: OneMinusSrcAlphaFactor, blendEquation: AddEquation,
    });
    const mesh = new Mesh(this.geometry, this.material);
    mesh.frustumCulled = false;
    this.scene.add(mesh);
    this.syncTheme();
    this.unsubscribeTheme = onThemeChange(() => this.syncTheme());
  }

  private syncTheme(): void {
    const accent = new Color(themeColor('--accent'));
    const u = this.material.uniforms;
    u.uA!.value = accent;
    u.uB!.value = accent.clone().offsetHSL(.09, .02, .08);
    u.uLight!.value = document.documentElement.dataset.theme === 'light' ? 1 : 0;
  }

  setPaused(paused: boolean): void { this.paused = paused; }

  draw(stage: GlStage, frame: Frame): void {
    const active = !this.paused && this.source.live;
    const values = this.curve.update(active ? this.source.frame() : this.zero, frame.dt);
    let energy = 0;
    for (let i = 0; i < values.length; i += 1) {
      this.samples[i] = DataUtils.toHalfFloat(values[i]!);
      energy += values[i]!;
    }
    // 无数据和静音只保留静息形体，艺术流速与音频平滑相互独立。
    if (active && energy > .01) this.time += frame.dt * .65;
    const target = active ? 1 : 0;
    this.gate += (target - this.gate) * (1 - Math.exp(-frame.dt / .12));
    this.texture.needsUpdate = true;
    this.material.uniforms.uTime!.value = this.time;
    this.material.uniforms.uGate!.value = this.gate;
    stage.renderer.render(this.scene, this.camera);
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.texture.dispose();
    this.material.dispose();
    this.geometry.dispose();
    this.scene.clear();
  }
}
