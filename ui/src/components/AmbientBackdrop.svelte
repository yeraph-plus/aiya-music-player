<script lang="ts">
  import { untrack } from 'svelte';
  import { detectCapability } from '../lib/render/capability';
  import { createGlStage, type GlStage } from '../lib/render/context';
  import { FlowLayer } from '../lib/render/layers/flow';
  import { RidgeLayer } from '../lib/render/layers/ridge';
  import { AtmosphereLayer } from '../lib/render/layers/atmosphere';
  import { createForeground, type ForegroundLayer } from '../lib/render/layers/foreground';
  import type { RenderHost } from '../lib/render/host.svelte';
  import { subscribeRender } from '../lib/render/scheduler';
  import { reducedMotion } from '../lib/motion.svelte';
  import type { BgSpectrumStyle, RidgeGlow } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props {
    host: RenderHost;
    source: SpectrumSource | null;
    enabled: boolean;
    shape: BgSpectrumStyle;
    /** 山形专属光效。 */
    glow: RidgeGlow;
    paused: boolean;
  }
  let { host, source, enabled, shape, glow, paused }: Props = $props();
  const glActive = $derived((enabled || Boolean(host.foreground)) && !reducedMotion.value && detectCapability().tier >= 2);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let stage = $state<GlStage | null>(null);
  let flow = $state<FlowLayer | null>(null);
  let spectrum = $state<RidgeLayer | AtmosphereLayer | null>(null);
  let foreground = $state<ForegroundLayer | null>(null);
  let lost = $state(false);

  $effect(() => {
    const target = canvas;
    if (!target || !glActive) return;
    const created = createGlStage(target);
    if (!created) return;
    stage = created;
    const onLost = () => { lost = true; };
    const onRestored = () => { lost = false; };
    target.addEventListener('webglcontextlost', onLost);
    target.addEventListener('webglcontextrestored', onRestored);
    const stopTicking = subscribeRender(now => created.render(now));
    return () => {
      stopTicking();
      target.removeEventListener('webglcontextlost', onLost);
      target.removeEventListener('webglcontextrestored', onRestored);
      stage = null;
      lost = false;
      created.dispose();
    };
  });

  $effect(() => {
    const current = stage;
    if (!current || !enabled) return;
    const field = new FlowLayer();
    current.add(field);
    flow = field;
    return () => { flow = null; current.remove(field); };
  });

  $effect(() => {
    const current = stage;
    const src = enabled ? source : null;
    const kind = shape;
    if (!current || !src) return;
    const layer = kind === 'ridge'
      ? new RidgeLayer(src, untrack(() => glow))
      : new AtmosphereLayer(src, kind);
    current.add(layer);
    spectrum = layer;
    return () => {
      spectrum = null;
      current.remove(layer);
    };
  });

  $effect(() => {
    const current = stage;
    const request = host.foreground;
    const src = request?.source;
    const kind = request?.style;
    const target = request?.target;
    if (!current || !src || !kind || !target) return;
    const layer = createForeground(src, kind);
    current.add(layer, target);
    foreground = layer;
    return () => { foreground = null; current.remove(layer); };
  });

  $effect(() => {
    const level = glow;
    if (spectrum instanceof RidgeLayer) spectrum.setGlow(level);
  });
  $effect(() => {
    flow?.setPaused(paused);
    spectrum?.setPaused(paused);
    foreground?.setPaused(host.foreground?.paused ?? true);
  });

  // 仅成功创建的动态舞台申请 tap；切换形态不重新订阅数据源。
  $effect(() => {
    const src = enabled ? source : null;
    if (!stage || lost || !src) return;
    src.start();
    return () => src.stop();
  });

  $effect(() => {
    const src = host.foreground?.source;
    if (!stage || lost || !src) return;
    src.start();
    return () => src.stop();
  });
</script>

<div class="ambient-bg" aria-hidden="true">
  <div class="ambient-art"></div>
  {#if glActive}
    <canvas bind:this={canvas} class="ambient-flow" style:visibility={lost ? 'hidden' : 'visible'}></canvas>
  {/if}
</div>

<style>
  .ambient-bg {
    position: absolute;
    inset: 0;
    z-index: var(--z-bg);
    overflow: hidden;
    pointer-events: none;
  }
  .ambient-art {
    position: absolute;
    inset: 0;
    background: var(--ambient-bg);
  }
  .ambient-flow {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
