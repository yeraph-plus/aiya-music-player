<script lang="ts">
  /**
   * 首页区块用的紧凑表格（**非虚拟化**，只渲染 8 行左右的预览）。
   *
   * 为什么不复用 `TrackList`：那个组件绑死在 `TrackWindow`（游标分页 +
   * 虚拟滚动 + 拖拽排序）上，首页这三段是"一次查 8 条"的快照，
   * 硬塞进去要伪造分页窗口。所以两边共用的是**交互模型**而不是组件：
   * 同一套勾选语义（点选框=切换、Shift=连选、表头=三态全选），
   * 同一套批量条（`BatchBar` 只认回调，不认下标）。
   *
   * 选择集按**曲目 ID**（不是下标）：首页三个区块各自是独立查询，
   * 下标在区块之间没有意义。
   */
  import { Check, Minus } from '@lucide/svelte';
  import { Checkbox } from 'bits-ui';
  import type { TrackRow } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import { formatDay, formatPlays, formatTime } from '../lib/format';
  import { rangeSelection, selectAll, toggleSelection } from '../lib/selection';

  interface Props {
    rows: TrackRow[];
    loading: boolean;
    /** 无数据时的文案 */
    empty: string;
    /** 第四列显示什么：时长 / 播放次数 / 入库日期 */
    valueKind: 'duration' | 'plays' | 'added';
    selected: ReadonlySet<string>;
    currentId: string | null;
    onchange: (ids: Set<string>) => void;
    onplay: (index: number) => void;
    onmenu?: (event: MouseEvent, index: number, track: TrackRow) => void;
  }

  let {
    rows,
    loading,
    empty,
    valueKind,
    selected,
    currentId,
    onchange,
    onplay,
    onmenu,
  }: Props = $props();

  /** Shift 连选的锚点（下标，仅在本表内有意义） */
  let anchor = $state(-1);

  const selectedIndices = $derived(
    rows.reduce<number[]>((out, row, index) => {
      if (selected.has(row.id)) out.push(index);
      return out;
    }, []),
  );
  const allSelected = $derived(rows.length > 0 && selectedIndices.length === rows.length);
  const someSelected = $derived(selectedIndices.length > 0 && !allSelected);

  function emit(indices: Set<number>): void {
    onchange(new Set([...indices].map((index) => rows[index]?.id).filter((id): id is string => !!id)));
  }

  /**
   * 点选框。
   *
   * 不给 Checkbox 传 `onCheckedChange`：它只回一个 boolean，拿不到 `shiftKey`
   * （连选要用）。勾选态**完全由 `checked` prop 决定**（受控，无本地副本），
   * 模型改了它就跟着改，不会出现"视觉上一个勾、模型里没选中"的错位。
   *
   * 拦"顺手起播"靠的不是 `stopPropagation`，而是**行自己认出点击来源**
   * （行里的 `closest('.tt-check')` 判断）：Svelte 5 把 click 委托到根节点
   * 统一派发，同一批 handler 之间 `stopPropagation` 不保证顺序。
   * 这里仍然 stop 一次，是为了不把事件继续交给外层的其它委托 handler。
   */
  function toggle(index: number, event: MouseEvent): void {
    event.stopPropagation();
    if (event.shiftKey && anchor >= 0) {
      emit(rangeSelection(anchor, index));
    } else {
      emit(toggleSelection(new Set(selectedIndices), index));
      anchor = index;
    }
  }

  function toggleAll(event: MouseEvent): void {
    event.stopPropagation();
    emit(allSelected ? new Set<number>() : selectAll(rows.length));
    anchor = allSelected ? -1 : 0;
  }

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
    <!-- 点击处理放在包装层而不是 Checkbox 上：bits-ui 自己的 click 处理器会同时跑，
         内部 checked 与我们的受控 prop 各翻一次，结果正好反相（实测：勾了的行
         显示未勾）。键盘可达性由内层 button 提供，这两条 a11y 提示是误报。 -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- tabindex=-1：可编程聚焦但不进 Tab 序列，真正的键盘入口是内层 button -->
    <span class="tt-check" role="columnheader" tabindex="-1" onclick={toggleAll}>
      <Checkbox.Root
        class="tt-box"
        checked={allSelected}
        indeterminate={someSelected}
        aria-label={allSelected ? '取消全选' : '全选'}
        disabled={rows.length === 0}
      >
        {#snippet children({ checked, indeterminate })}
          {#if indeterminate}<Minus size={11} />{:else if checked}<Check size={11} />{/if}
        {/snippet}
      </Checkbox.Root>
    </span>
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
      class:selected={selected.has(track.id)}
      role="row"
      tabindex="-1"
      onclick={(event) => {
        // 勾选取的是"点勾选框"，不是"点这一行"（播放）。Svelte 5 的 click 是
        // 委托到根节点派发的，`stopPropagation` 拦不住同一批 handler —— 行自己
        // 判一次来源才可靠。
        if ((event.target as HTMLElement | null)?.closest('.tt-check')) return;
        onplay(index);
      }}
      onkeydown={(event) => {
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
      <!-- 同上：点选框由包装层接管，键盘可达性在 button 上 -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span class="tt-check" role="cell" tabindex="-1" onclick={(event) => toggle(index, event)}>
        <Checkbox.Root
          class="tt-box"
          checked={selected.has(track.id)}
          aria-label={`选择 ${track.title}`}
        >
          {#snippet children({ checked })}
            {#if checked}<Check size={11} />{/if}
          {/snippet}
        </Checkbox.Root>
      </span>
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
