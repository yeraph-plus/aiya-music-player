<script lang="ts">
  /**
   * 首页区块用的紧凑表格（**非虚拟化**，只渲染 8 行左右的预览）。
   *
   * 为什么不复用 `TrackList`：那个组件绑死在 `TrackWindow`（游标分页 +
   * 虚拟滚动 + 拖拽排序）上，首页这三段是"一次查 8 条"的快照，
   * 硬塞进去要伪造分页窗口。
   *
   * docs/27 起是**纯预览表**：勾选列与批量操作撤掉（批量归主列表的
   * 工具条），行 hover、双击播放、右键菜单保留；第四列显示什么由
   * `valueKind` 决定（时长 / 播放次数 / 入库日期）。
   */
  import type { TrackRow } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { formatDay, formatPlays, formatTime } from '../lib/format';

  interface Props {
    rows: TrackRow[];
    loading: boolean;
    /** 无数据时的文案 */
    empty: string;
    /** 第四列显示什么：时长 / 播放次数 / 入库日期 */
    valueKind: 'duration' | 'plays' | 'added';
    currentId: string | null;
    /**
     * 播放。**只由双击 / 键盘 Enter 触发**（B4-1 交互语义，与主列表一致）；
     * 紧凑表是预览性质，不设 hover 播放键。
     */
    onplay: (index: number) => void;
    onmenu?: (event: MouseEvent, index: number, track: TrackRow) => void;
  }

  let { rows, loading, empty, valueKind, currentId, onplay, onmenu }: Props = $props();

  function value(row: TrackRow): string {
    if (valueKind === 'plays') return formatPlays(row.playCount);
    if (valueKind === 'added') return formatDay(row.addedAtMs);
    return formatTime(row.durationMs);
  }

  const valueTitle = (row: TrackRow): string =>
    valueKind === 'added' ? new Date(row.addedAtMs).toLocaleString('zh-CN') : value(row);
</script>

<div class="tt" role="table" aria-label="曲目列表">
  <div class="tt-head" role="row">
    <span role="columnheader"></span>
    <span role="columnheader">标题</span>
    <span role="columnheader">艺人</span>
    <span role="columnheader">专辑</span>
    <span class="tt-num" role="columnheader">
      {valueKind === 'plays' ? '次数' : valueKind === 'added' ? '入库' : '时长'}
    </span>
  </div>

  {#each rows as track, index (track.id)}
    <div
      class="tt-row"
      class:active={currentId === track.id}
      role="row"
      tabindex="-1"
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
      {#if track.artworkId}
        <img class="cover sm" src={artworkUrl(track.artworkId, 96) ?? ''} alt="" loading="lazy" />
      {:else}
        <span class="cover sm placeholder">♪</span>
      {/if}
      <span class="cell" title={track.title}>{track.title}</span>
      <span class="cell dim" title={track.artist ?? ''}>{track.artist ?? '未知艺人'}</span>
      <span class="cell dim" title={track.album ?? ''}>{track.album ?? '—'}</span>
      <span class="cell tt-num" title={valueTitle(track)}>{value(track)}</span>
    </div>
  {:else}
    {#if loading}
      <div class="tt-skeleton"></div>
      <div class="tt-skeleton"></div>
      <div class="tt-skeleton"></div>
    {:else}
      <div class="tt-empty">{empty}</div>
    {/if}
  {/each}
</div>
