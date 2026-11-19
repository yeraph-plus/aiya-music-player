import { AddEquation, Color, CustomBlending, DataTexture, DataUtils, HalfFloatType, LinearFilter, Mesh, OneFactor, OneMinusSrcAlphaFactor, OrthographicCamera, PlaneGeometry, RedFormat, Scene, ShaderMaterial } from 'three';
import { onThemeChange, themeColor } from '../../accent';
import type { SpectrumSource } from '../../spectrum-source';
import type { SpectrumStyle } from '../../settings';
import type { Frame, GlLayer, GlStage } from '../context';
import { SoftSpectrum } from '../soft-spectrum';
import { CONTOUR, LATTICE, ORBIT, RIBBON, SHORE, STRING } from './foreground-shaders';

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() { vUv = uv; gl_Position = vec4(position.xy, 0., 1.); }
`;
const COMMON = /* glsl */ `
  varying vec2 vUv;
  uniform float uTime, uBass, uMid, uHigh, uGain, uAspect, uGate, uLight;
  uniform vec3 uA, uB;
  uniform sampler2D uBands, uSoft;
  float opacity = 0.;
  float band(float x) { return texture2D(uBands, vec2(clamp(x,.01,.99),.5)).r; }
  float softBand(float x) { return texture2D(uSoft, vec2(clamp(x,0.,1.),.5)).r; }
  float glow(float d, float width) { return exp(-abs(d)/width); }
  mat2 rot(float a) { return mat2(cos(a),-sin(a),sin(a),cos(a)); }
  vec3 srgb(vec3 c) {
    return mix(c*12.92, 1.055*pow(max(c,vec3(0.)),vec3(1./2.4))-.055,step(vec3(.0031308),c));
  }
  vec3 ink(float a, float b) {
    opacity += a;
    vec3 color = mix(uA,uB,clamp(b,0.,1.));
    return srgb(mix(color,color*.48,uLight))*a;
  }
`;

/** 前景共用固定采样缓冲，子类只提供各自形态，舞台负责区域与 GPU 所有权。 */
export class ForegroundLayer implements GlLayer {
  private readonly scene = new Scene();
  private readonly camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly geometry = new PlaneGeometry(2, 2);
  private readonly smooth = new SoftSpectrum();
  private readonly bands = new Float32Array(48);
  private readonly bandData = new Uint16Array(48);
  private readonly softData = new Uint16Array(256);
  private readonly zero = new Float32Array(48);
  private readonly bandTexture: DataTexture;
  private readonly softTexture: DataTexture;
  private readonly material: ShaderMaterial;
  private readonly unsubscribeTheme: () => void;
  private time = 0;
  private gate = 0;
  private paused = false;

  constructor(readonly name: SpectrumStyle, private readonly source: SpectrumSource, body: string) {
    this.bandTexture = this.makeTexture(this.bandData);
    this.softTexture = this.makeTexture(this.softData);
    this.material = new ShaderMaterial({
      vertexShader: VERT,
      fragmentShader: COMMON + /* glsl */ `
        void main() {
          vec2 p=(vUv-vec2(.5,.43))*vec2(uAspect,1.);
          vec3 c=vec3(0.);
          ${body}
          float alpha = min(opacity,.82);
          gl_FragColor = vec4(c*(alpha/max(opacity,.00001)),alpha);
        }
      `,
      uniforms: {
        uTime: { value: 0 }, uBass: { value: 0 }, uMid: { value: 0 }, uHigh: { value: 0 },
        uGate: { value: 0 }, uGain: { value: 1 }, uAspect: { value: 1 }, uLight: { value: 0 },
        uA: { value: new Color() }, uB: { value: new Color() },
        uBands: { value: this.bandTexture }, uSoft: { value: this.softTexture },
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

  private makeTexture(data: Uint16Array): DataTexture {
    const texture = new DataTexture(data, data.length, 1, RedFormat, HalfFloatType);
    texture.minFilter = texture.magFilter = LinearFilter;
    texture.generateMipmaps = false;
    texture.needsUpdate = true;
    return texture;
  }

  private syncTheme(): void {
    const accent = new Color(themeColor('--accent')), u = this.material.uniforms;
    u.uA!.value = accent;
    u.uB!.value = accent.clone().offsetHSL(.09,.02,.08);
    u.uLight!.value = document.documentElement.dataset.theme === 'light' ? 1 : 0;
  }

  resize(width: number, height: number): void {
    this.material.uniforms.uAspect!.value = width / Math.max(1, height);
  }

  setPaused(paused: boolean): void { this.paused = paused; }

  draw(stage: GlStage, frame: Frame): void {
    const active = !this.paused && this.source.live;
    const input = active ? this.source.frame() : this.zero;
    const values = this.smooth.update(input, frame.dt);
    const k = 1 - Math.exp(-frame.dt / .09);
    let bass = 0, mid = 0, high = 0;
    for (let i = 0; i < 48; i += 1) {
      const raw = input[i] ?? 0;
      const value = Number.isFinite(raw) ? Math.min(1,Math.max(0,raw)) : 0;
      this.bands[i] += (value-this.bands[i]!) * k;
      this.bandData[i] = DataUtils.toHalfFloat(this.bands[i]!);
      if (i < 10) bass += this.bands[i]!;
      else if (i < 30) mid += this.bands[i]!;
      else high += this.bands[i]!;
    }
    for (let i = 0; i < 256; i += 1) this.softData[i] = DataUtils.toHalfFloat(values[i]!);
    if (active && bass + mid + high > .001) this.time += frame.dt * 1.5;
    this.gate += ((active ? 1 : 0)-this.gate) * (1-Math.exp(-frame.dt/.12));
    const u = this.material.uniforms;
    u.uTime!.value = this.time; u.uGate!.value = this.gate;
    u.uBass!.value = bass/10; u.uMid!.value = mid/20; u.uHigh!.value = high/18;
    this.bandTexture.needsUpdate = this.softTexture.needsUpdate = true;
    stage.renderer.render(this.scene,this.camera);
  }

  dispose(): void {
    this.unsubscribeTheme();
    this.bandTexture.dispose(); this.softTexture.dispose();
    this.material.dispose(); this.geometry.dispose(); this.scene.clear();
  }
}

export class OrbitLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('orbit',source,ORBIT); }
}
export class RibbonLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('ribbon',source,RIBBON); }
}
export class LatticeLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('lattice',source,LATTICE); }
}
export class ContourLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('contour',source,CONTOUR); }
}
export class StringLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('string',source,STRING); }
}
export class ShoreLayer extends ForegroundLayer {
  constructor(source: SpectrumSource) { super('shore',source,SHORE); }
}

const LAYERS = { orbit: OrbitLayer, ribbon: RibbonLayer, lattice: LatticeLayer,
  contour: ContourLayer, string: StringLayer, shore: ShoreLayer };

export function createForeground(source: SpectrumSource, style: SpectrumStyle): ForegroundLayer {
  return new LAYERS[style](source);
}
