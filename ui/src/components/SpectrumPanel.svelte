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
   * compact 引擎：数据来自 `source`（48 带 tap），本组件只做绘制：
   * 单个 rAF 循环 + canvas。暂停/无数据时源返回平滑到 0 的历史值，
   * 呈现为"衰减归零"而不是冻结或跳变。
   *
   * pro 引擎：自研五形态渲染器（audiomotion.ts），同样吃 `source` 的
   * 48 带 tap，只是渲染更讲究（渐变/LED/镜像/径向/反射）。
   * 2026-10-25 起两种引擎数据同源，差别只在渲染层——audioMotion 需要
   * WebAudio 节点而 WebView 没有音频图（ADR-0006），已移除。
   */
  let canvas = $state<HTMLCanvasElement | null>(null);
  let proHost = $state<HTMLElement | null>(null);
  let proMode = $state<ProSpectrumMode>(PRO_MODES[0] as ProSpectrumMode);
  let frameId = 0;
  /** 引擎实例存在 effect 闭包里；这里只持引用供按钮回调使用（非响应式） */
  let proHandle: ProSpectrumHandle | null = null;

  const usePro = $derived(engine === 'pro');

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

  // pro 引擎随面板挂载/卸载建毁。
  // 注意 effect 里不能读 proMode（会变成依赖 → 每次切形态整个引擎重建），
  // 形态由 handle 内部持有，这里只把 chip 状态与它对齐。
  $effect(() => {
    if (!usePro) return;
    const host = proHost;
    if (!host) return;
    const created = createProSpectrum(host, source, 'bars');
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
