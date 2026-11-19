<script lang="ts">
  import { untrack } from 'svelte';
  import { useRenderHost } from '../lib/render/host.svelte';
  import type { SpectrumStyle } from '../lib/settings';
  import type { SpectrumSource } from '../lib/spectrum-source';

  interface Props { source: SpectrumSource | null; style: SpectrumStyle; paused: boolean }
  let { source, style, paused }: Props = $props();
  const host = useRenderHost();
  let target = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!target || !source) return;
    const request = { target, source, style: untrack(() => style), paused: untrack(() => paused) };
    host.foreground = request;
    return () => { host.foreground = null; };
  });
  $effect(() => {
    const kind = style, stopped = paused;
    untrack(() => {
      if (!host.foreground) return;
      host.foreground.style = kind;
      host.foreground.paused = stopped;
    });
  });
</script>

{#if source}
  <div bind:this={target} class="np-spectrum" aria-hidden="true"></div>
{/if}

<style>
  .np-spectrum {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
</style>
