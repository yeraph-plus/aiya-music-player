<script lang="ts">
  /**
   * 首页区块用的静态预览列表（**非虚拟化**，只渲染十几行的快照）。
   *
   * 为什么不复用 `TrackList`：那个组件绑死在 `TrackWindow`（游标分页 +
   * 虚拟滚动 + 拖拽排序）上，首页的预览是"一次查 N 条"的快照，
   * 硬塞进去要伪造分页窗口。
   *
   * 2026-10-28 用户拍板：预览列表与主列表**同外观**——直接复用 lists.css
   * 的 `.row` / `.grid-head`（`.list-shell.no-batch` 网格模板），此前的
   * `.tt-*` 独立表格（列宽节奏与主列表不同，肉眼可见的行列错位）撤销。
   * 差异只剩：无批量勾选列、无滚动容器（纯显示，页面级滚动）、
   * 无排序表头（预览的顺序是语义的一部分）。
   *
   * 列 = 主列表非批量档的"必要列"：# / 封面 / 标题 / 歌手 / 专辑 / 数值。
   * 第四列显示什么由 `valueKind` 决定（时长 / 播放次数 / 入库日期）。
   */
  import { Play } from '@lucide/svelte';
  import type { TrackRow } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { formatDay, formatPlays, formatTime } from '../lib/format';
  import { player } from '../stores/player.svelte';
  import Cover from './ui/Cover.svelte';
  import EmptyState from './ui/EmptyState.svelte';

  interface Props {
    rows: TrackRow[];
    loading: boolean;
    /** 无数据时的文案 */
    empty: string;
    /** 最后一列显示什么：时长 / 播放次数 / 入库日期 */
    valueKind: 'duration' | 'plays' | 'added';
    currentId: string | null;
    /** 播放：双击 / Enter / 封面悬浮播放键触发 */
    onplay: (index: number) => void;
    onmenu?: (event: MouseEvent, index: number, track: TrackRow) => void;
    /** 歌手/专辑跳转（与主列表对齐）。给了回调且行里有 id 才渲染成链接 */
    onopenartist?: (id: string) => void;
    onopenalbum?: (id: string) => void;
  }

  let { rows, loading, empty, valueKind, currentId, onplay, onmenu, onopenartist, onopenalbum }: Props = $props();

  /** 正在播放的行：# 列换成动态声条（与主列表同一套 .bars 样式） */
  const playing = $derived(player.state?.status === 'playing');

  function value(row: TrackRow): string {
    if (valueKind === 'plays') return formatPlays(row.playCount);
    if (valueKind === 'added') return formatDay(row.addedAtMs);
    return formatTime(row.durationMs);
  }

  const valueTitle = (row: TrackRow): string =>
    valueKind === 'added' ? new Date(row.addedAtMs).toLocaleString('zh-CN') : value(row);
</script>

<div class="list-shell no-batch preview-list" role="table" aria-label="曲目列表">
  <div class="grid-head tt-head" role="row">
    <span class="idx" role="columnheader">#</span>
    <span role="columnheader"></span>
    <span role="columnheader">标题</span>
    <span role="columnheader">歌手</span>
    <span role="columnheader">专辑</span>
    <span class="num" role="columnheader">
      {valueKind === 'plays' ? '次数' : valueKind === 'added' ? '入库' : '时长'}
    </span>
  </div>

  {#each rows as track, index (track.id)}
    <div
      class="row"
      class:active={currentId === track.id}
      role="row"
      tabindex="0"
      ondblclick={(event) => {
        onplay(index);
        void event;
      }}
      onkeydown={(event) => {
        if ((event.target as HTMLElement | null)?.closest('button')) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onplay(index);
        }
      }}
      oncontextmenu={(event) => {
        event.preventDefault();
        onmenu?.(event, index, track);
      }}
    >
      <span class="idx">
        {#if currentId === track.id}
          <span class="bars" class:animated={playing} aria-label="正在播放">
            <i></i><i></i><i></i>
          </span>
        {:else}
          <span class="idx-num">{index + 1}</span>
        {/if}
      </span>
      <span class="cover-slot">
        {#if track.artworkId}
          <Cover size="row" src={artworkUrl(track.artworkId, 96)} alt="" />
        {:else}
          <Cover size="row" alt="" />
        {/if}
        <button
          class="play-btn"
          tabindex="-1"
          aria-label={`播放 ${track.title}`}
          onclick={() => onplay(index)}
        >
          <Play size={13} />
        </button>
      </span>
      <span class="cell title" title={track.title}>{track.title}</span>
      <span class="cell dim">
        {#if track.artistId && track.artist && onopenartist}
          <button
            class="link-cell"
            title={`查看歌手 ${track.artist}`}
            onclick={(event) => {
              event.stopPropagation();
              onopenartist(track.artistId!);
            }}
          >
            {track.artist}
          </button>
        {:else}
          <span title={track.artist ?? ''}>{track.artist ?? '未知艺人'}</span>
        {/if}
      </span>
      <span class="cell dim">
        {#if track.albumId && track.album && onopenalbum}
          <button
            class="link-cell"
            title={`查看专辑 ${track.album}`}
            onclick={(event) => {
              event.stopPropagation();
              onopenalbum(track.albumId!);
            }}
          >
            {track.album}
          </button>
        {:else}
          <span title={track.album ?? ''}>{track.album ?? '—'}</span>
        {/if}
      </span>
      <span class="cell num" title={valueTitle(track)}>{value(track)}</span>
    </div>
  {:else}
    {#if loading}
      <div class="tt-skeleton"></div>
      <div class="tt-skeleton"></div>
      <div class="tt-skeleton"></div>
    {:else}
      <EmptyState title={empty} />
    {/if}
  {/each}
</div>

<style>
  /* 预览是页面流里的一段，不参与主列表的"占满剩余高度 + 内部滚动"：
     表头不吸顶、外壳不限高，内容随页面滚动 */
  .preview-list {
    flex: none;
  }

  .tt-head {
    position: static;
  }
</style>
