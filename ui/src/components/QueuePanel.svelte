<script lang="ts">
  import type { TrackDetail } from '../lib/ipc';
  import type { TrackRef } from '../lib/ipc/generated/TrackRef';
  import { artworkUrl } from '../lib/artwork';
  import { startRowDrag, consumeClickSuppressed } from '../lib/drag';
  import { createVirtualWindow } from '../lib/create-virtual-window.svelte';
  import { formatDurationLong, formatTime, isStreamId, transientOrigin } from '../lib/format';
  import { library as libraryStore } from '../stores/library.svelte';
  import Cover from './ui/Cover.svelte';
  import EmptyState from './ui/EmptyState.svelte';
  import IconButton from './ui/IconButton.svelte';
import Button from './ui/Button.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    ids: string[];
    /**
     * 队列里的临时项（拖入的文件 / 直播流）。
     *
     * 元数据不能靠 `library_tracks` 取——它们不在曲库里，查了必然空手而归，
     * 还会把 id 记进负缓存。标题与来源只能由快照自带的这一份给出
     * （`QueueSnapshot.transient`，docs/52 §7.2）。
     */
    transient: TrackRef[];
    /** 当前播放位置（播放顺序下标）；空队列 = null（契约：不用 -1 哨兵） */
    index: number | null;
    onplay: (index: number) => void;
    onremove: (index: number) => void;
    onmove: (from: number, to: number) => void;
    onclear: () => void;
    /** 播放中与否：决定当前行的声条是否跳动 */
    playing: boolean;
    /** 关闭（头部的 × 键；Esc/外点由外层 SideDrawer 处理，不走这里） */
    onclose: () => void;
  }

  let { ids, index, transient, playing, onplay, onremove, onmove, onclear, onclose }: Props = $props();

  /** id → 临时项。通常是空 Map（队列全是库内曲目时） */
  const transientById = $derived(new Map(transient.map((ref) => [ref.id, ref])));

  const ROW_HEIGHT = 46;
  /**
   * 滚动内容里行区之前的占位：`.queue-list` 自身的 padding-top
   * （player.css 6px）。插入线是 scroller 内的绝对定位元素、随内容滚，
   * 定位是内容坐标——不补这 6px 线就会贴在目标行上沿之上一点。
   */
  const CONTENT_TOP = 6;
  const META_BATCH = 120;
  /** 同一 id 确认失败/缺失达到该次数后不再请求（有界负缓存，防无限重试） */
  const META_ATTEMPT_LIMIT = 3;
  /** 负缓存条数上限，超过整体清空（曲目可能重新上架，允许再试） */
  const META_FAILURE_CAP = 1000;

  let metaVersion = $state(0);
  let dragState = $state<{ from: number; to: number; active: boolean } | null>(null);

  // 队列可能上万首，面板里也只渲染可见的那几十行。
  // 元数据按可见区间批量取，而不是"拉全队列的曲目"。
  const meta = new Map<string, TrackDetail>();
  const pending = new Set<string>();
  /** trackId → 已失败次数：返回缺失（可能已从曲库删除）或请求异常时累加 */
  const failedAttempts = new Map<string, number>();

  function noteFailure(id: string): void {
    if (failedAttempts.size >= META_FAILURE_CAP) failedAttempts.clear();
    failedAttempts.set(id, (failedAttempts.get(id) ?? 0) + 1);
  }

  /** 滚动水管与 TrackList 共用（scroller/视口测量/区间），这里只接业务 effect */
  const vw = createVirtualWindow({
    rowHeight: () => ROW_HEIGHT,
    total: () => ids.length,
    overscan: 4,
  });

  const range = $derived(vw.range);

  const visible = $derived.by(() => {
    void metaVersion;
    const out: Array<{
      index: number;
      id: string;
      track: TrackDetail | undefined;
      transient: TrackRef | undefined;
    }> = [];
    for (let i = range.start; i < range.end; i += 1) {
      const id = ids[i];
      if (!id) continue;
      out.push({ index: i, id, track: meta.get(id), transient: transientById.get(id) });
    }
    return out;
  });

  const totalMs = $derived.by(() => {
    void metaVersion;
    // 元数据随滚动补齐：还有行没加载时总时长必然偏小且漂移，宁可不算。
    // 按 ids 逐个查 meta（而非 meta.size），避免历史残留条目虚报完成度。
    if (ids.length === 0) return 0;
    let sum = 0;
    let known = 0;
    for (const id of ids) {
      // 临时项的时长来自快照（拖入的文件由引擎回填；直播恒为 null = 0）
      const item = transientById.get(id);
      if (item) {
        known += 1;
        sum += item.durationMs ?? 0;
        continue;
      }
      const track = meta.get(id);
      if (!track) continue;
      known += 1;
      sum += track.durationMs ?? 0;
    }
    return known === ids.length ? sum : 0;
  });

  /** 插入线纵坐标 = 行边界 + 内容顶部占位（内容坐标，与滚动位置无关） */
  const dropLineTop = $derived(
    dragState && dragState.active ? CONTENT_TOP + dragState.to * ROW_HEIGHT : -1,
  );

  $effect(() => {
    const missing: string[] = [];
    for (const row of visible) {
      if (row.track || row.transient || pending.has(row.id)) continue;
      // 负缓存命中：确认过多次缺失的 id 不再请求，否则 effect 会无限重跑
      if ((failedAttempts.get(row.id) ?? 0) >= META_ATTEMPT_LIMIT) continue;
      missing.push(row.id);
      if (missing.length >= META_BATCH) break;
    }
    if (missing.length === 0) return;

    for (const id of missing) pending.add(id);
    void libraryStore
      .tracks(missing)
      .then((tracks) => {
        for (const id of missing) pending.delete(id);
        const got = new Set(tracks.map((track) => track.id));
        for (const track of tracks) meta.set(track.id, track);
        // 返回里没有的 id 视为确认缺失，记入负缓存
        for (const id of missing) {
          if (!got.has(id)) noteFailure(id);
        }
        metaVersion += 1;
      })
      .catch(() => {
        // 请求本身失败：清 pending 并计入失败，同样有界重试
        for (const id of missing) {
          pending.delete(id);
          noteFailure(id);
        }
        metaVersion += 1;
      });
  });

  // 队列换了（重新播放）时把面板滚回当前曲目附近
  $effect(() => {
    const target = index;
    if (!vw.scroller || target == null) return;
    const top = CONTENT_TOP + target * ROW_HEIGHT;
    if (top < vw.scroller.scrollTop || top > vw.scroller.scrollTop + vw.viewportHeight - ROW_HEIGHT) {
      vw.scroller.scrollTop = Math.max(0, top - vw.viewportHeight / 3);
      vw.scrollTop = vw.scroller.scrollTop;
    }
  });

  function handlePointerDown(event: PointerEvent, rowIndex: number): void {
    startRowDrag({
      event,
      index: rowIndex,
      count: ids.length,
      rowHeight: ROW_HEIGHT,
      scroller: vw.scroller,
      onState: (next) => (dragState = next),
      onDrop: (from, to) => onmove(from, to),
    });
  }
</script>

<!--
  壳体（B5a）：本组件只渲染**内容**。外层浮层（定位/Esc/外点关闭/焦点还原/
  aria-expanded）由 PlayerBar 里队列键的 kit `Popover` 负责 —— 此前是
  `<svelte:window>` 手写外点 + Esc 两件套，且触发键双触发问题要靠
  `closest('[data-queue-trigger]')` 自防。
-->
<div class="queue-panel queue-float" aria-label="播放队列">
  <header>
    <strong>播放队列</strong>
    <span class="dim">{ids.length} 首{totalMs > 0 ? ` · ${formatDurationLong(totalMs)}` : ''}</span>
    <Button variant="link" onclick={onclear} disabled={ids.length === 0}>清空</Button>
    <IconButton label="关闭队列" size="sm" onclick={onclose}>
      <Icon name="close" size={13} />
    </IconButton>
  </header>

  <div
    class="queue-list"
    class:dragging={dragState?.active}
    bind:this={vw.scroller}
    onscroll={vw.handleScroll}
  >
    {#if ids.length === 0}
      <EmptyState title="队列是空的" hint="右键曲目可加入队列" />
    {:else}
      <div style={`height:${range.padTop}px`}></div>
      {#each visible as row (row.index)}
        <div
          class="queue-row"
          class:active={row.index === index}
          class:drag-source={dragState?.active && dragState.from === row.index}
          onclick={(event) => {
            // 拖拽激活后松手会派发一次残留 click，吞掉避免误播放
            if (consumeClickSuppressed()) {
              event.stopPropagation();
              return;
            }
            onplay(row.index);
          }}
          onpointerdown={(event) => handlePointerDown(event, row.index)}
          role="button"
          tabindex="0"
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              onplay(row.index);
            } else if (event.key === 'Delete') {
              onremove(row.index);
            }
          }}
        >
          <span class="q-grip" aria-hidden="true"><Icon name="grip" size={13} /></span>
          <span class="q-index" role="cell">
            {#if row.index === index}
              <span class="bars" class:animated={playing === true} aria-label="正在播放">
                <i></i><i></i><i></i>
              </span>
            {:else}
              {row.index + 1}
            {/if}
          </span>
          <Cover src={row.track?.artworkId ? artworkUrl(row.track.artworkId, 96) : null} size="sm" alt="" />
          {#if row.transient}
            <!--
              临时项（docs/52 §7.2）：标题来自快照，副行是**来源标记 + 出处**。
              这里刻意没有任何"跳到专辑/艺人"的入口——`artistId`/`albumId` 都是
              null，做成链接点下去只会得到一页空白。
            -->
            <span class="q-meta">
              <span class="q-title">{row.transient.title}</span>
              <span class="q-artist">
                <span class="q-tag">{isStreamId(row.id) ? '直播' : '文件'}</span>
                {transientOrigin(row.id) ?? ''}
              </span>
            </span>
            <span class="q-duration">
              {isStreamId(row.id) ? '直播' : formatTime(row.transient.durationMs ?? 0)}
            </span>
          {:else}
            <span class="q-meta">
              <span class="q-title">{row.track?.title ?? '载入中…'}</span>
              <span class="q-artist">{row.track?.artist ?? ''}</span>
            </span>
            <span class="q-duration">{formatTime(row.track?.durationMs ?? 0)}</span>
          {/if}
          <IconButton
            label="从队列移除"
            size="sm"
            onclick={(event) => {
              event.stopPropagation();
              onremove(row.index);
            }}
          >
            <Icon name="close" size={12} />
          </IconButton>
        </div>
      {/each}
      <div style={`height:${range.padBottom}px`}></div>
      {#if dropLineTop >= 0}
        <div class="drop-line" style={`top:${dropLineTop}px`} aria-hidden="true"></div>
      {/if}
    {/if}
  </div>
</div>
