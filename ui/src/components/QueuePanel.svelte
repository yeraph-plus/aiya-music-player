<script lang="ts">
  import type { Track } from '@aiya/contracts';
  import { artworkUrl } from '@aiya/contracts';
  import { startRowDrag } from '../lib/drag';
  import { computeRange } from '../lib/virtual';
  import { formatDurationLong, formatTime } from '../lib/format';
  import { library as libraryStore } from '../stores/library.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    ids: string[];
    index: number;
    onplay: (index: number) => void;
    onremove: (index: number) => void;
    onmove: (from: number, to: number) => void;
    onclear: () => void;
    onclose: () => void;
  }

  let { ids, index, onplay, onremove, onmove, onclear, onclose }: Props = $props();

  const ROW_HEIGHT = 46;
  const META_BATCH = 120;

  let scroller = $state<HTMLDivElement | null>(null);
  let viewportHeight = $state(520);
  let scrollTop = $state(0);
  let metaVersion = $state(0);
  let dragState = $state<{ from: number; to: number; active: boolean } | null>(null);

  // 队列可能上万首，面板里也只渲染可见的那几十行。
  // 元数据按可见区间批量取，而不是"拉全队列的曲目"。
  const meta = new Map<string, Track>();
  const pending = new Set<string>();

  const range = $derived(
    computeRange({ scrollTop, viewportHeight, rowHeight: ROW_HEIGHT, total: ids.length, overscan: 4 }),
  );

  const visible = $derived.by(() => {
    void metaVersion;
    const out: Array<{ index: number; id: string; track: Track | undefined }> = [];
    for (let i = range.start; i < range.end; i += 1) {
      const id = ids[i];
      if (!id) continue;
      out.push({ index: i, id, track: meta.get(id) });
    }
    return out;
  });

  const totalMs = $derived.by(() => {
    void metaVersion;
    let sum = 0;
    for (const track of meta.values()) sum += track.durationMs ?? 0;
    return sum;
  });

  const dropLineTop = $derived(
    dragState && dragState.active ? range.padTop + dragState.to * ROW_HEIGHT : -1,
  );

  $effect(() => {
    const missing: string[] = [];
    for (const row of visible) {
      if (row.track || pending.has(row.id)) continue;
      missing.push(row.id);
      if (missing.length >= META_BATCH) break;
    }
    if (missing.length === 0) return;

    for (const id of missing) pending.add(id);
    void libraryStore.tracks(missing).then((tracks) => {
      for (const id of missing) pending.delete(id);
      for (const track of tracks) meta.set(track.id, track);
      metaVersion += 1;
    });
  });

  // 队列换了（重新播放）时把面板滚回当前曲目附近
  $effect(() => {
    const target = index;
    if (!scroller || target < 0) return;
    const top = target * ROW_HEIGHT;
    if (top < scroller.scrollTop || top > scroller.scrollTop + scroller.clientHeight - ROW_HEIGHT) {
      scroller.scrollTop = Math.max(0, top - scroller.clientHeight / 3);
      scrollTop = scroller.scrollTop;
    }
  });

  $effect(() => {
    if (!scroller) return;
    const observer = new ResizeObserver(() => {
      viewportHeight = scroller?.clientHeight ?? 520;
    });
    observer.observe(scroller);
    viewportHeight = scroller.clientHeight;
    return () => observer.disconnect();
  });

  function handlePointerDown(event: PointerEvent, rowIndex: number): void {
    startRowDrag({
      event,
      index: rowIndex,
      count: ids.length,
      rowHeight: ROW_HEIGHT,
      scroller,
      onState: (next) => (dragState = next),
      onDrop: (from, to) => onmove(from, to),
    });
  }
</script>

<aside class="queue-panel" aria-label="播放队列">
  <header>
    <strong>播放队列</strong>
    <span class="dim">{ids.length} 首{totalMs > 0 ? ` · ${formatDurationLong(totalMs)}` : ''}</span>
    <button class="link" onclick={onclear} disabled={ids.length === 0}>清空</button>
    <button class="icon-btn" onclick={onclose} aria-label="关闭队列"><Icon name="close" size={14} /></button>
  </header>

  <div
    class="queue-list"
    class:dragging={dragState?.active}
    bind:this={scroller}
    onscroll={() => {
      if (scroller) scrollTop = scroller.scrollTop;
    }}
  >
    {#if ids.length === 0}
      <div class="queue-empty">队列是空的<br /><span class="dim">右键曲目可加入队列</span></div>
    {:else}
      <div style={`height:${range.padTop}px`}></div>
      {#each visible as row (row.index)}
        <div
          class="queue-row"
          class:active={row.index === index}
          class:drag-source={dragState?.active && dragState.from === row.index}
          onclick={() => onplay(row.index)}
          onpointerdown={(event) => handlePointerDown(event, row.index)}
          role="button"
          tabindex="-1"
          onkeydown={(event) => {
            if (event.key === 'Enter') onplay(row.index);
          }}
        >
          <span class="q-grip" aria-hidden="true"><Icon name="grip" size={13} /></span>
          <span class="q-index">{row.index === index ? '▶' : row.index + 1}</span>
          {#if row.track?.artworkId}
            <img class="cover sm" src={artworkUrl(row.track.artworkId, 96) ?? ''} alt="" loading="lazy" />
          {:else}
            <span class="cover sm placeholder">♪</span>
          {/if}
          <span class="q-meta">
            <span class="q-title">{row.track?.title ?? '载入中…'}</span>
            <span class="q-artist">{row.track?.artist ?? ''}</span>
          </span>
          <span class="q-duration">{formatTime(row.track?.durationMs ?? 0)}</span>
          <button
            class="icon-btn tiny"
            aria-label="从队列移除"
            onclick={(event) => {
              event.stopPropagation();
              onremove(row.index);
            }}
          >
            <Icon name="close" size={12} />
          </button>
        </div>
      {/each}
      <div style={`height:${range.padBottom}px`}></div>
      {#if dropLineTop >= 0}
        <div class="drop-line" style={`top:${dropLineTop}px`} aria-hidden="true"></div>
      {/if}
    {/if}
  </div>
</aside>
