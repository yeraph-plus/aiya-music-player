<script lang="ts">
  import { Funnel, FunnelPlus, HeartPlus, ListChecks, ListPlus, LocateFixed } from '@lucide/svelte';
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';

  /**
   * 曲目列表工具条（docs/27）：挂在 TrackList 顶部（不随列表滚动）。
   *
   * 左组是动作，**全部内置**，替代旧悬浮 BatchBar；右组是列表自身的
   * 两个模式件：筛选器（开模态框）与批量操作开关（控制勾选列显隐）。
   * 批量动作对「当前选择集」生效——批量关着时也可以单击选中一行再用，
   * 所以按钮的禁用条件只看 `count`，不看开关。
   *
   * docs/28 图标化：筛选器与批量操作收成**纯图标按钮**（语义由 aria-label
   * 与 title 承担），右组两个常驻控件不再各占一截文字宽度；筛选生效时
   * 图标从 Funnel 换成 FunnelPlus 并点主色，扫一眼就知道筛着东西。
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
    /** 是否有正在播放的曲目（「定位到正在播放」的显隐条件；没有就不渲染按钮） */
    canLocate?: boolean;
    onplayall: () => void;
    onenqueue: () => void;
    onaddto: () => void;
    onremove: () => void;
    onfilteropen: () => void;
    onbatchmode: (on: boolean) => void;
    /** 跳到正在播放的条目（一次性定位，不做持续自动跟随） */
    onlocate: () => void;
  }

  let {
    total,
    count,
    removeLabel,
    batchMode,
    filterCount,
    canFilter,
    canLocate = false,
    onplayall,
    onenqueue,
    onaddto,
    onremove,
    onfilteropen,
    onbatchmode,
    onlocate,
  }: Props = $props();
</script>

<div class="list-toolbar" role="toolbar" aria-label="列表操作">
  <Button size="sm" variant="primary" disabled={total === 0} onclick={onplayall}>
    {#snippet icon()}<Icon name="play" size={12} />{/snippet}
    播放全部
  </Button>
  <Button size="sm" disabled={count === 0} onclick={onenqueue}>
    {#snippet icon()}<ListPlus size={13} />{/snippet}
    加入播放队列
  </Button>
  <Button size="sm" disabled={count === 0} onclick={onaddto}>
    {#snippet icon()}<HeartPlus size={13} />{/snippet}
    加入歌单
  </Button>
  <Button size="sm" variant="danger" disabled={count === 0} onclick={onremove}>
    {#snippet icon()}<Icon name="trash" size={12} />{/snippet}
    {removeLabel}
  </Button>

  {#if count > 0}
    <span class="lt-count">已选 {count} 首</span>
  {/if}

  <span class="lt-space"></span>

  {#if canLocate}
    <!-- 定位到正在播放：纯手动触发（docs/27 右组模式件同款图标键）。
         不做自动跟随——播放中列表被反复拽回播放行是修掉的老 bug。 -->
    <IconButton label="定位到正在播放" size="sm" onclick={onlocate}>
      <LocateFixed size={15} />
    </IconButton>
  {/if}

  {#if canFilter}
    <span class="lt-filter" class:on={filterCount > 0}>
      <IconButton
        label={filterCount > 0 ? `筛选器（${filterCount} 档生效）` : '筛选器'}
        size="sm"
        active={filterCount > 0}
        onclick={onfilteropen}
      >
        {#if filterCount > 0}
          <FunnelPlus size={15} />
        {:else}
          <Funnel size={15} />
        {/if}
      </IconButton>
    </span>
  {/if}
  <IconButton
    label="批量操作"
    size="sm"
    pressed={batchMode}
    onclick={() => onbatchmode(!batchMode)}
  >
    <ListChecks size={15} />
  </IconButton>
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

  .lt-filter :global(.k-icon-btn.on) {
    background: var(--accent-weak);
    color: var(--accent-text);
  }
</style>
