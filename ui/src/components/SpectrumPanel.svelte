<script lang="ts">
  import Icon from './Icon.svelte';
  import type { SpectrumSource } from '../lib/spectrum-source';
  import {
    createProSpectrum,
    PRO_MODES,
    PRO_MODE_LABEL,
    type ProSpectrumHandle,
    type ProSpectrumMode,
  } from '../lib/audiomotion';

  interface Props {
    source: SpectrumSource;
    /** compact：自研轻量条形；pro：audioMotion 专业渲染（形态可循环切换） */
    engine: 'compact' | 'pro';
    onclose: () => void;
  }

  let { source, engine, onclose }: Props = $props();

  /**
   * 频谱面板（ADR-037）。
   *
   * compact 引擎：数据来自 `source`（AudioSink 的 AnalyserSource），本组件只做绘制：
   * 单个 rAF 循环 + canvas。暂停/无数据时源返回平滑到 0 的历史值，
   * 呈现为"衰减归零"而不是冻结或跳变。
   *
   * pro 引擎：audioMotion 直接挂接 `source.audioNode`（只读直通节点），
   * 自带 rAF 循环与 HiDPI 处理；头部按钮循环切换形态。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);
  let proHost = $state<HTMLElement | null>(null);
  let proMode = $state<ProSpectrumMode>(PRO_MODES[0] as ProSpectrumMode);
  let frameId = 0;
  /** 引擎实例存在 effect 闭包里；这里只持引用供按钮回调使用（非响应式） */
  let proHandle: ProSpectrumHandle | null = null;

  /**
   * 能不能真的用 pro。
   *
   * audioMotion 必须挂一个**真实音频节点**（`source.audioNode`），而 ADR-0006
   * 之后音频在 Rust sidecar 出声，`RemoteSpectrumSource.audioNode` 恒为
   * `undefined`。默认设置又是 `spectrumEngine: 'pro'` —— 于是"开频谱"
   * 曾经得到一个**完全空白**的面板（2026-10-25 在浏览器预览里实测到）。
   *
   * 判据放在这里而不是让用户去设置里改：注释本来就写着"audioNode 缺失时
   * 静默降级"，现在把降级落到实处 —— 有数据（48 带 tap）就以 compact 画出来，
   * 空面板才是真降级失败。
   */
  const usePro = $derived(engine === 'pro' && source.audioNode !== undefined);

  /** 峰值帽：每根柱子的当前峰值，无数据时缓慢下落 */
  let peaks: Float32Array = new Float32Array(0);
  const PEAK_DROP = 0.008;

  function draw(): void {
    const target = canvas;
    if (!target) return;
    const ctx = target.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const width = target.clientWidth;
    const height = target.clientHeight;
    if (width === 0 || height === 0) {
      frameId = requestAnimationFrame(draw);
      return;
    }
    if (target.width !== Math.round(width * dpr) || target.height !== Math.round(height * dpr)) {
      target.width = Math.round(width * dpr);
      target.height = Math.round(height * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const bands = source.frame();
    if (peaks.length !== bands.length) peaks = new Float32Array(bands.length);

    const styles = getComputedStyle(target);
    const barColor = styles.getPropertyValue('--accent').trim() || '#7c9cff';
    const peakColor = styles.getPropertyValue('--text-dim').trim() || 'rgba(128,128,128,.8)';

    const count = bands.length;
    const slot = width / count;
    const barWidth = Math.max(2, slot - 1);

    for (let band = 0; band < count; band += 1) {
      const value = Math.min(1, bands[band] as number);
      const x = band * slot;
      const barHeight = value * (height - 4);

      ctx.fillStyle = barColor;
      ctx.fillRect(x, height - barHeight, barWidth, barHeight);

      // 峰值帽
      const peak = (peaks[band] as number) ?? 0;
      const nextPeak = value >= peak ? value : Math.max(0, peak - PEAK_DROP);
      peaks[band] = nextPeak;
      if (nextPeak > 0.005) {
        ctx.fillStyle = peakColor;
        ctx.fillRect(x, Math.max(0, height - nextPeak * (height - 4) - 4), barWidth, 2);
      }
    }

    frameId = requestAnimationFrame(draw);
  }

  // 数据源与 compact 绘制循环的生命周期（pro 模式下引擎自带循环）
  $effect(() => {
    source.start();
    if (usePro) {
      return () => source.stop();
    }
    frameId = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(frameId);
      source.stop();
    };
  });

  // pro 引擎随面板挂载/卸载建毁；没有 audioNode 时不会走到这里（usePro 已为 false）
  $effect(() => {
    if (!usePro) return;
    const host = proHost;
    const node = source.audioNode;
    if (!host || !node) return;
    const created = createProSpectrum(host, node);
    proHandle = created;
    proMode = created.mode;
    return () => {
      proHandle = null;
      created.destroy();
    };
  });

  function cycleProMode(): void {
    if (!proHandle) return;
    proMode = proHandle.cycleMode();
  }
</script>

<aside class="spectrum-panel" aria-label="频谱">
  <header>
    <strong>频谱</strong>
    {#if usePro}
      <button class="mode-chip" onclick={cycleProMode} title="点击切换形态">
        {PRO_MODE_LABEL[proMode]}
      </button>
    {:else}
      <span class="dim">实时</span>
    {/if}
    <button class="icon-btn" onclick={onclose} aria-label="关闭频谱">
      <Icon name="close" size={14} />
    </button>
  </header>
  {#if usePro}
    <div class="spectrum-body pro-body">
      <div class="pro-host" bind:this={proHost}></div>
    </div>
  {:else}
    <div class="spectrum-body">
      <canvas bind:this={canvas}></canvas>
    </div>
  {/if}
</aside>

<style>
  .spectrum-body {
    min-height: 0;
    display: flex;
    padding: 10px 14px 14px;
  }
  .pro-body {
    padding: 4px 6px 8px;
  }
  .pro-host {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .mode-chip {
    margin-right: auto;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text-dim);
    font-size: 11px;
    padding: 2px 10px;
    cursor: pointer;
  }
  .mode-chip:hover {
    border-color: var(--accent);
  }
  canvas {
    flex: 1;
    width: 100%;
    height: 100%;
  }
</style>
