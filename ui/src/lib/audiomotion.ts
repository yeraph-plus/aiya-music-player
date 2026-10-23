import { AudioMotionAnalyzer } from 'audiomotion-analyzer';

/**
 * audioMotion-analyzer 适配层（AGPL-3.0，选型见 docs/09）。
 *
 * 职责边界：
 * - 音频图的所有权仍在 AudioSink，这里只把它的 AnalyserNode 作为只读输入挂进来
 *   （`connectSpeakers: false` 保证引擎绝不触碰 destination，音频路径零改动）
 * - 主题色通过 registerGradient 注入，观感与应用一致
 * - 引擎生命周期随面板开/关建毁，不留常驻对象
 *
 * 形态集是刻意挑过的"装饰观感"子集，不是 audioMotion 全部能力。
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

/** 各形态的差异配置，公共配置在创建参数里 */
const MODE_OPTIONS: Record<ProSpectrumMode, object> = {
  bars: { ledBars: false, lumiBars: false, radial: false, mirror: 0, reflexRatio: 0, showPeaks: true },
  led: { ledBars: true, trueLeds: true, lumiBars: false, radial: false, mirror: 0, reflexRatio: 0, showPeaks: false },
  mirror: { ledBars: false, lumiBars: false, radial: false, mirror: 1, reflexRatio: 0, showPeaks: true },
  radial: { ledBars: false, lumiBars: false, radial: true, mirror: 0, reflexRatio: 0, showPeaks: false, spinSpeed: 0 },
  reflex: { ledBars: false, lumiBars: false, radial: false, mirror: 0, reflexRatio: 0.4, reflexAlpha: 1, reflexFit: true, showPeaks: true },
};

export interface ProSpectrumHandle {
  readonly mode: ProSpectrumMode;
  /** 切换形态；传入不在集合里的值会被忽略 */
  setMode(mode: ProSpectrumMode): void;
  /** 循环切到下一个形态，返回新形态 */
  cycleMode(): ProSpectrumMode;
  destroy(): void;
}

export function createProSpectrum(
  container: HTMLElement,
  audioNode: AudioNode,
  initialMode: ProSpectrumMode = 'bars',
): ProSpectrumHandle {
  const styles = getComputedStyle(container);
  const accent = styles.getPropertyValue('--accent').trim() || '#7c9cff';

  const analyzer = new AudioMotionAnalyzer(container, {
    source: audioNode,
    connectSpeakers: false,
    start: true,
    stereo: false,
    gradient: 'aiya-accent',
    barSpace: 2,
    // 面板背景已经是主题色，引擎不要自己铺底
    bgAlpha: 0,
    showBgColor: false,
    minFreq: 30,
    maxFreq: 16000,
  });

  analyzer.registerGradient('aiya-accent', {
    colorStops: [
      mix(accent, '#000000', 0.45),
      { pos: 0.6, color: accent },
      { pos: 1, color: mix(accent, '#ffffff', 0.55) },
    ],
  });

  let mode: ProSpectrumMode = initialMode;
  const apply = (next: ProSpectrumMode): void => {
    mode = next;
    Object.assign(analyzer, MODE_OPTIONS[next]);
  };
  apply(mode);

  return {
    get mode(): ProSpectrumMode {
      return mode;
    },
    setMode(next: ProSpectrumMode): void {
      if (PRO_MODES.includes(next)) apply(next);
    },
    cycleMode(): ProSpectrumMode {
      const index = PRO_MODES.indexOf(mode);
      const next = PRO_MODES[(index + 1) % PRO_MODES.length] as ProSpectrumMode;
      apply(next);
      return next;
    },
    destroy(): void {
      // 库没有显式 dispose：断开输入、停掉动画，DOM 随容器一起移除
      try {
        analyzer.disconnectInput(audioNode);
      } catch {
        /* 已断开或从未连接，忽略 */
      }
      analyzer.stop();
      container.replaceChildren();
    },
  };
}
