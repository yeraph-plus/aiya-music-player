<script lang="ts">
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import Switch from './ui/Switch.svelte';

  /**
   * 曲目列表工具条（docs/27）：挂在 TrackList 顶部（不随列表滚动）。
   *
   * 左组是动作，**全部内置**，替代旧悬浮 BatchBar；右组是列表自身的
   * 两个模式件：筛选器（开模态框）与批量操作开关（控制勾选列显隐）。
   * 批量动作对「当前选择集」生效——批量关着时也可以单击选中一行再用，
   * 所以按钮的禁用条件只看 `count`，不看开关。
   */
  interface Props {
    /** 当前列表总数（「播放全部」的禁用条件） */
    total: number;
    /** 当前选择集行数（批量动作的禁用条件与「已选 N 首」展示） */
    count: number;
    /** 移除按钮文案随视图变：曲库=从曲库中删除，歌单=从播放列表移除 */
    removeLabel: string;
    batchMode: boolean;
    /** 时间筛选生效中的档位数（筛选键角标） */
    filterCount: number;
    /** 筛选器是否可用（歌单等固定顺序视图不提供） */
    canFilter: boolean;
    onplayall: () => void;
    onenqueue: () => void;
    onaddto: () => void;
    onremove: () => void;
    onfilteropen: () => void;
    onbatchmode: (on: boolean) => void;
  }

  let {
    total,
    count,
    removeLabel,
    batchMode,
    filterCount,
    canFilter,
    onplayall,
    onenqueue,
    onaddto,
    onremove,
    onfilteropen,
    onbatchmode,
  }: Props = $props();
</script>

<div class="list-toolbar" role="toolbar" aria-label="列表操作">
  <Button size="sm" variant="primary" disabled={total === 0} onclick={onplayall}>
    {#snippet icon()}<Icon name="play" size={12} />{/snippet}
    播放全部
  </Button>
  <Button size="sm" disabled={count === 0} onclick={onenqueue}>加入播放队列</Button>
  <Button size="sm" disabled={count === 0} onclick={onaddto}>加入歌单</Button>
  <Button size="sm" variant="danger" disabled={count === 0} onclick={onremove}>
    {#snippet icon()}<Icon name="trash" size={12} />{/snippet}
    {removeLabel}
  </Button>

  {#if count > 0}
    <span class="lt-count">已选 {count} 首</span>
  {/if}

  <span class="lt-space"></span>

  {#if canFilter}
    <button
      class="lt-filter"
      class:on={filterCount > 0}
      onclick={onfilteropen}
      title="按入库时间、上次播放筛选"
    >
      筛选器{filterCount > 0 ? ` · ${filterCount}` : ''}
    </button>
  {/if}
  <label class="lt-batch">
    <Switch
      label="批量操作模式"
      checked={batchMode}
      onchange={(next) => onbatchmode(next)}
    />
    批量操作
  </label>
</div>

<style>
  .list-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 22px 8px;
  }

  .lt-count {
    font-size: 11.5px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .lt-space {
    flex: 1;
  }

  .lt-filter {
    border: 1px solid var(--line-strong);
    background: transparent;
    color: var(--text-2);
    font-size: 12px;
    border-radius: 999px;
    padding: 4px 12px;
    cursor: pointer;
  }

  .lt-filter:hover {
    color: var(--text-1);
    border-color: var(--accent);
  }

  .lt-filter.on {
    color: var(--accent-text);
    border-color: var(--accent);
    background: var(--accent-weak);
  }

  .lt-batch {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-2);
    cursor: pointer;
    user-select: none;
  }
</style>
