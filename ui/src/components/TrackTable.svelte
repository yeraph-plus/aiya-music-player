<script lang="ts">
  /**
   * 通用静态曲目表格：非虚拟化，一次渲染调用方给的快照（首页常听等小数据集）。
   *
   * 不复用 `TrackList`：后者绑定 `TrackWindow`（游标分页 + 虚拟滚动 + 拖拽
   * 排序），而这里是快照列表，接入需伪造分页窗口。
   *
   * **列可配置**：`columns` 决定列序与列集，网格模板按列定义动态拼出
   * （内联 style 覆盖 lists.css 的固定模板——后者仍被主列表使用，不能为
   * 这里改动）。可选列：序号 / 封面 / 标题 / 歌手 / 专辑 / 数值列
   * （次数 / 时长 / 入库日期 / 最近播放四选一，kind 即内容）。
   *
   * 无批量勾选列、无滚动容器（页面级滚动承担）、无排序表头（行序是
   * 调用方语义的一部分）。外观与主列表同源：`.row` / `.grid-head` 类。
   */
  import { Play } from '@lucide/svelte';
  import type { TrackRow } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { formatDateTime, formatDay, formatPlays, formatTime } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { player } from '../stores/player.svelte';
  import Cover from './ui/Cover.svelte';
  import EmptyState from './ui/EmptyState.svelte';

  /** 可选列：数值列（plays/duration/added）三选一，kind 即列内容 */
  export type Column =
    | 'index'
    | 'cover'
    | 'title'
    | 'artist'
    | 'album'
    | 'plays'
    | 'duration'
    | 'added'
    | 'lastPlayed';

  interface Props {
    rows: TrackRow[];
    /** 列序即渲染序；表头文案与网格宽度都由它派生 */
    columns: Column[];
    loading: boolean;
    /** 无数据时的文案 */
    empty: string;
    currentId: string | null;
    /** 播放：双击 / Enter / 封面悬浮播放键触发 */
    onplay: (index: number) => void;
    onmenu?: (event: MouseEvent, index: number, track: TrackRow) => void;
    /** 歌手/专辑跳转（与主列表对齐）。给了回调且行里有 id 才渲染成链接 */
    onopenartist?: (id: string) => void;
    onopenalbum?: (id: string) => void;
  }

  let { rows, columns, loading, empty, currentId, onplay, onmenu, onopenartist, onopenalbum }: Props = $props();

  /** 正在播放的行：# 列换成动态声条（与主列表同一套 .bars 样式） */
  const playing = $derived(player.state?.status === 'playing');

  /** 列 → 网格宽度。与 lists.css 主列表同源量级，fr 列吃剩余空间 */
  const GRID: Record<Column, string> = {
    index: '34px',
    cover: '42px',
    title: 'minmax(120px, 2.2fr)',
    artist: 'minmax(80px, 1.3fr)',
    album: 'minmax(80px, 1.3fr)',
    plays: '56px',
    duration: '56px',
    added: '80px',
    lastPlayed: '80px',
  };

  /** 列 → 表头文案（$derived：语言切换时跟随）。cover 列无表头 */
  const LABEL: Record<Column, string> = $derived({
    index: '#',
    cover: '',
    title: t('tracks.colTitle'),
    artist: t('tracks.colArtist'),
    album: t('tracks.colAlbum'),
    plays: t('tracks.colPlays'),
    duration: t('tracks.colDuration'),
    added: t('tracks.colAdded'),
    lastPlayed: t('tracks.colLastPlayed'),
  });

  const isValue = (col: Column): boolean =>
  col === 'plays' || col === 'duration' || col === 'added' || col === 'lastPlayed';

  function value(row: TrackRow, col: Column): string {
    if (col === 'plays') return formatPlays(row.playCount);
    if (col === 'added') return formatDay(row.addedAtMs);
    if (col === 'lastPlayed') return row.lastPlayedAtMs === null ? '—' : formatDay(row.lastPlayedAtMs);
    return formatTime(row.durationMs);
  }

  const valueTitle = (row: TrackRow, col: Column): string =>
    col === 'added'
      ? new Date(row.addedAtMs).toLocaleString('zh-CN')
      : col === 'lastPlayed' && row.lastPlayedAtMs !== null
        ? formatDateTime(row.lastPlayedAtMs)
        : value(row, col);

  const gridStyle = $derived(columns.map((col) => GRID[col]).join(' '));
</script>

<div class="list-shell no-batch preview-list" role="table" aria-label={t('tracks.tableLabel')}>
  <div class="grid-head tt-head" role="row" style:grid-template-columns={gridStyle}>
    {#each columns as col (col)}
      <span class:idx={col === 'index'} class:num={isValue(col)} role="columnheader">{LABEL[col]}</span>
    {/each}
  </div>

  {#each rows as track, index (track.id)}
    <div
      class="row"
      class:active={currentId === track.id}
      role="row"
      tabindex="0"
      style:grid-template-columns={gridStyle}
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
      {#each columns as col (col)}
        {#if col === 'index'}
          <span class="idx">
            {#if currentId === track.id}
              <span class="bars" class:animated={playing} aria-label={t('tracks.nowPlaying')}>
                <i></i><i></i><i></i>
              </span>
            {:else}
              <span class="idx-num">{index + 1}</span>
            {/if}
          </span>
        {:else if col === 'cover'}
          <span class="cover-slot">
            {#if track.artworkId}
              <Cover size="row" src={artworkUrl(track.artworkId, 96)} alt="" />
            {:else}
              <Cover size="row" alt="" />
            {/if}
            <button
              class="play-btn"
              tabindex="-1"
              aria-label={t('tracks.playTrack', { title: track.title })}
              onclick={() => onplay(index)}
            >
              <Play size={13} />
            </button>
          </span>
        {:else if col === 'title'}
          <span class="cell title" title={track.title}>{track.title}</span>
        {:else if col === 'artist'}
          <span class="cell dim">
            {#if track.artistId && track.artist && onopenartist}
              <button
                class="link-cell"
                title={t('tracks.viewArtist', { name: track.artist })}
                onclick={(event) => {
                  event.stopPropagation();
                  onopenartist(track.artistId!);
                }}
              >
                {track.artist}
              </button>
            {:else}
              <span title={track.artist ?? ''}>{track.artist ?? t('shell.player.unknownArtist')}</span>
            {/if}
          </span>
        {:else if col === 'album'}
          <span class="cell dim">
            {#if track.albumId && track.album && onopenalbum}
              <button
                class="link-cell"
                title={t('tracks.viewAlbum', { name: track.album })}
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
        {:else}
          <span class="cell num" title={valueTitle(track, col)}>{value(track, col)}</span>
        {/if}
      {/each}
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
  /* 页面流里的一段，不参与主列表的"占满剩余高度 + 内部滚动"：
     表头不吸顶、外壳不限高，内容随页面滚动 */
  .preview-list {
    flex: none;
  }

  /* 表头不要主列表那条半透明滚动带（首页不吸顶、没有滚过亮封面的问题）：
     一条下横线标出列表顶界即可 */
  .tt-head {
    position: static;
    background: none;
    backdrop-filter: none;
    border-bottom: 1px solid var(--line);
  }

  /* 最后一列（次数/时长/入库/最近播放这类短数值）居中：列宽只有 56-80px，
     右对齐时数字贴壳缘、左对齐时贴封面列，居中在窄列里最稳 */
  .tt-head span:last-child,
  .preview-list .row > span:last-child {
    text-align: center;
  }
</style>
