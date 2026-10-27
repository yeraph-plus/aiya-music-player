import type { SpectrumSource } from './spectrum-source';
import { onThemeChange, themeColor } from './accent';

/**
 * pro 频谱渲染器（2026-10-25 重写，原 audioMotion-analyzer 适配层已删）。
 *
 * **为什么自研**：audioMotion 必须挂一个真实的 WebAudio 节点，而 ADR-0006
 * 之后音频在 Rust sidecar 出声、ADR-0003 禁止 PCM 进 WebView——
 * `RemoteSpectrumSource.audioNode` 恒为 undefined，pro 引擎**从架构上就挂不上**，
 * 只能降级成 compact（2026-10-25 之前 pro 设置实际是摆设，选型记录见 docs/09）。
 * WASAPI loopback 也救不了：loopback 数据仍落在 Rust 侧，喂给 UI 里的
 * audioMotion 照样要传 PCM。
 *
 * 所以 pro 的"专业感"只能来自**渲染层**而不是数据源：同一份 48 带 tap，
 * 五种形态（渐变条/LED/镜像/径向/反射）用 canvas 直接画。副产物：
 * audiomotion-analyzer 依赖移除，第三方 notices 清单随之减一项
 * （licenses/THIRD-PARTY-NOTICES.md）。
 *
 * 生命周期随面板开/关建毁：rAF 循环、ResizeObserver 都在 destroy() 里拆。
 */

export type ProSpectrumMode = 'bars' | 'led' | 'mirror' | 'radial' | 'reflex';

export const PRO_MODES: readonly ProSpectrumMode[] = ['bars', 'led', 'mirror', 'radial', 'reflex'];

export const PRO_MODE_LABEL: Record<ProSpectrumMode, string> = {
  bars: '渐变条',
  led: 'LED',
  mirror: '镜像',
  radial: '径向',
  reflex: '反射',
};

export interface ProSpectrumHandle {
  readonly mode: ProSpectrumMode;
  setMode(mode: ProSpectrumMode): void;
  cycleMode(): ProSpectrumMode;
  destroy(): void;
}

/** 主题强调色的明暗变体：hex → 简易混合，非 hex 输入原样返回 */
function mix(hex: string, other: string, ratio: number): string {
  const parse = (value: string): [number, number, number] | null => {
    const match = /^#([0-9a-f]{6})$/i.exec(value.trim());
    if (!match) return null;
    const int = parseInt(match[1] as string, 16);
    return [(int >> 16) & 0xff, (int >> 8) & 0xff, int & 0xff];
  };
  const from = parse(hex);
  const to = parse(other);
  if (!from || !to) return hex;
  const channel = (a: number, b: number): number => Math.round(a + (b - a) * ratio);
  const [r, g, b] = [
    channel(from[0] as number, to[0] as number),
    channel(from[1] as number, to[1] as number),
    channel(from[2] as number, to[2] as number),
  ];
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`;
}

/** 峰值帽下落速度（每帧，0-1 值域） */
const PEAK_DROP = 0.01;
/** LED 形态的点阵密度（px） */
const LED_CELL = 7;
const LED_GAP = 2;
/** 径向形态的内圈半径占比 */
const RADIAL_INNER = 0.28;
/** 反射形态的实体区高度占比（其余给倒影） */
const REFLEX_BODY = 0.62;
/** 渐变贴图的高度（像素）：绘制时纵向拉伸到柱高，256 档足够平滑 */
const GRAD_STEPS = 256;

/**
 * 竖直渐变贴图（1×256），绘制时 `drawImage` 拉伸。
 *
 * 为什么不是每根柱子 `createLinearGradient`：那是每帧每柱一个新
 * `CanvasGradient` 对象（bars/reflex 形态 48 柱 × 每帧，60fps 下每秒
 * 数千次分配），而渐变的两端颜色只在切主题时变。贴图一次建好、每帧只是
 * 一次 GPU 纹理拉伸——观感一致（渐变的插值方向本来就是纵向均匀的）。
 */
function makeGradientSheet(bottom: string, top: string): HTMLCanvasElement {
  const sheet = document.createElement('canvas');
  sheet.width = 1;
  sheet.height = GRAD_STEPS;
  const c = sheet.getContext('2d');
  if (c) {
    const g = c.createLinearGradient(0, GRAD_STEPS, 0, 0);
    g.addColorStop(0, bottom);
    g.addColorStop(1, top);
    c.fillStyle = g;
    c.fillRect(0, 0, 1, GRAD_STEPS);
  }
  return sheet;
}

export function createProSpectrum(
  container: HTMLElement,
  source: SpectrumSource,
  initialMode: ProSpectrumMode = 'bars',
): ProSpectrumHandle {
  const canvas = document.createElement('canvas');
  canvas.style.cssText = 'display:block;width:100%;height:100%;';
  container.replaceChildren(canvas);
  const ctx = canvas.getContext('2d');

  // 主题色只在切主题/换背景方案时变，取一次缓存住（见 accent.ts）。
  // 这几项随主题重建，所以是 let 而不是 const。
  let accent = themeColor('--accent');
  // 渐变：暗底 → 主题色 → 亮顶，观感对齐原 audioMotion 的 aiya-accent 渐变
  let gradBottom = mix(accent, '#000000', 0.45);
  let gradTop = mix(accent, '#ffffff', 0.55);
  let peakColor = themeColor('--text-2');
  let unlitColor = themeColor('--text-3');
  let gradSheet = makeGradientSheet(gradBottom, gradTop);

  /** 换主题：重算颜色并重建贴图（面板常驻，不能等下次打开才更新） */
  const stopThemeWatch = onThemeChange(() => {
    accent = themeColor('--accent');
    gradBottom = mix(accent, '#000000', 0.45);
    gradTop = mix(accent, '#ffffff', 0.55);
    peakColor = themeColor('--text-2');
    unlitColor = themeColor('--text-3');
    gradSheet = makeGradientSheet(gradBottom, gradTop);
  });

  let mode: ProSpectrumMode = PRO_MODES.includes(initialMode) ? initialMode : 'bars';
  let frameId = 0;
  let disposed = false;
  /** 峰值帽状态随模式保留（bars/led/mirror/reflex 共用） */
  let peaks = new Float32Array(source.bandCount);

  const ro = new ResizeObserver(() => resize());
  ro.observe(container);

  function resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w === 0 || h === 0) return;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }

  function draw(): void {
    if (disposed || !ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w === 0 || h === 0) {
      frameId = requestAnimationFrame(draw);
      return;
    }
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) resize();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const bands = source.frame();
    const count = bands.length;
    if (peaks.length !== count) peaks = new Float32Array(count);

    // 峰值帽每帧统一推进，各形态共用
    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      peaks[i] = v >= peaks[i]! ? v : Math.max(0, peaks[i]! - PEAK_DROP);
    }

    if (mode === 'radial') drawRadial(w, h, count, bands);
    else if (mode === 'led') drawLed(w, h, count, bands);
    else if (mode === 'mirror') drawMirror(w, h, count, bands);
    else drawBars(w, h, count, bands, mode === 'reflex');

    frameId = requestAnimationFrame(draw);
  }

  /** 竖直渐变填充（暗底 → 亮顶）：拉伸贴图，不新建渐变对象 */
  function fillGradient(x: number, y: number, w: number, h: number): void {
    ctx!.drawImage(gradSheet, 0, 0, 1, GRAD_STEPS, x, y, w, h);
  }

  function drawBars(w: number, h: number, count: number, bands: Float32Array, reflex: boolean): void {
    const bodyH = reflex ? h * REFLEX_BODY : h;
    const slot = w / count;
    const barW = Math.max(2, slot - 2);
    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      const x = i * slot + (slot - barW) / 2;
      const bh = v * (bodyH - 4);
      if (bh > 0.5) {
        fillGradient(x, bodyH - bh, barW, bh);
      }
      const pk = peaks[i]!;
      if (pk > 0.005) {
        ctx!.fillStyle = peakColor;
        ctx!.fillRect(x, Math.max(0, bodyH - pk * (bodyH - 4) - 3), barW, 2);
      }
      if (reflex && bh > 0.5) {
        // 倒影：实体区下方，高度压缩 + alpha 衰减
        const rh = Math.min(bh * 0.55, (h - bodyH) * 0.9);
        if (rh > 0.5) {
          ctx!.globalAlpha = 0.28;
          fillGradient(x, bodyH + 2, barW, rh);
          ctx!.globalAlpha = 1;
        }
      }
    }
  }

  function drawLed(w: number, h: number, count: number, bands: Float32Array): void {
    const slot = w / count;
    const barW = Math.max(LED_CELL - LED_GAP, 3);
    const cols = Math.floor((h - 4) / LED_CELL);
    const dimFrom = Math.floor(cols * 0.55); // 越高越亮的分界
    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      const x = i * slot + (slot - barW) / 2;
      const lit = Math.round(v * cols);
      const peakRow = Math.round(peaks[i]! * cols);
      for (let row = 0; row < cols; row += 1) {
        const y = h - 4 - (row + 1) * LED_CELL + LED_GAP;
        if (row < lit) {
          ctx!.fillStyle = row >= dimFrom ? gradTop : accent;
          ctx!.globalAlpha = 0.55 + 0.45 * (row / cols);
        } else if (peakRow > 0 && row === peakRow) {
          ctx!.fillStyle = peakColor;
          ctx!.globalAlpha = 0.9;
        } else {
          ctx!.fillStyle = unlitColor;
          ctx!.globalAlpha = 0.35;
        }
        ctx!.fillRect(x, y, barW, LED_CELL - LED_GAP);
        ctx!.globalAlpha = 1;
      }
    }
  }

  function drawMirror(w: number, h: number, count: number, bands: Float32Array): void {
    const slot = w / count;
    const barW = Math.max(2, slot - 2);
    const mid = h / 2;
    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      const x = i * slot + (slot - barW) / 2;
      const bh = (v * (h - 6)) / 2;
      if (bh > 0.5) {
        // 上半亮（渐变），下半暗（同一色的暗变体），中轴对称
        fillGradient(x, mid - bh, barW, bh);
        ctx!.fillStyle = gradBottom;
        ctx!.globalAlpha = 0.6;
        ctx!.fillRect(x, mid, barW, bh);
        ctx!.globalAlpha = 1;
      }
      const pk = (peaks[i]! * (h - 6)) / 2;
      if (pk > 1) {
        ctx!.fillStyle = peakColor;
        ctx!.fillRect(x, mid - pk - 2, barW, 2);
        ctx!.fillRect(x, mid + pk, barW, 2);
      }
    }
  }

  function drawRadial(w: number, h: number, count: number, bands: Float32Array): void {
    const cx = w / 2;
    const cy = h / 2;
    const inner = Math.min(w, h) * RADIAL_INNER;
    const maxLen = Math.min(w, h) / 2 - inner - 2;
    // 内圈底环
    ctx!.strokeStyle = unlitColor;
    ctx!.lineWidth = 2;
    ctx!.beginPath();
    ctx!.arc(cx, cy, inner - 3, 0, Math.PI * 2);
    ctx!.stroke();

    const grad = ctx!.createLinearGradient(cx, cy - inner - maxLen, cx, cy);
    grad.addColorStop(0, gradTop);
    grad.addColorStop(1, gradBottom);

    for (let i = 0; i < count; i += 1) {
      const v = Math.min(1, bands[i] as number);
      const angle = (i / count) * Math.PI * 2 - Math.PI / 2;
      const len = v * maxLen;
      const x1 = cx + Math.cos(angle) * inner;
      const y1 = cy + Math.sin(angle) * inner;
      const x2 = cx + Math.cos(angle) * (inner + len);
      const y2 = cy + Math.sin(angle) * (inner + len);
      ctx!.strokeStyle = len > 0.5 ? grad : unlitColor;
      ctx!.lineWidth = Math.max(2, (Math.PI * 2 * inner) / count - 3);
      ctx!.beginPath();
      ctx!.moveTo(x1, y1);
      ctx!.lineTo(x2, y2);
      ctx!.stroke();

      const pk = peaks[i]! * maxLen;
      if (pk > 1) {
        ctx!.fillStyle = peakColor;
        ctx!.beginPath();
        ctx!.arc(
          cx + Math.cos(angle) * (inner + pk),
          cy + Math.sin(angle) * (inner + pk),
          1.6,
          0,
          Math.PI * 2,
        );
        ctx!.fill();
      }
    }
  }

  frameId = requestAnimationFrame(draw);

  return {
    get mode(): ProSpectrumMode {
      return mode;
    },
    setMode(next: ProSpectrumMode): void {
      if (PRO_MODES.includes(next)) mode = next;
    },
    cycleMode(): ProSpectrumMode {
      mode = PRO_MODES[(PRO_MODES.indexOf(mode) + 1) % PRO_MODES.length] as ProSpectrumMode;
      return mode;
    },
    destroy(): void {
      disposed = true;
      cancelAnimationFrame(frameId);
      ro.disconnect();
      stopThemeWatch();
      container.replaceChildren();
    },
  };
}
