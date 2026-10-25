<script lang="ts">
  import type { TrackSort } from '../lib/query';
  import Icon from './Icon.svelte';

  /**
   * 顶栏：搜索、排序、库统计与扫描入口。
   *
   * 纯展示组件：自己不含任何查询状态，输入变化直接回调给 App。
   * 这样"搜索防抖""切换排序"这类行为只有一处实现，组件不需要知道。
   */

  interface Props {
    search: string;
    sort: TrackSort;
    direction: 'asc' | 'desc';
    canSort: boolean;
    total: number;
    scanning: boolean;
    onsearch: (value: string) => void;
    onclearsearch: () => void;
    onsort: () => void;
    onsettings: () => void;
    onchooseroot: () => void;
    onscan: () => void;
    onimport: () => void;
  }

  let {
    search,
    sort,
    direction,
    canSort,
    total,
    scanning,
    onsearch,
    onclearsearch,
    onsort,
    onsettings,
    onchooseroot,
    onscan,
    onimport,
  }: Props = $props();

  let input = $state<HTMLInputElement | null>(null);

  /** 快捷键"聚焦搜索"用：把焦点交回给这里的输入框 */
  export function focus(): void {
    input?.focus();
  }
</script>

<div class="topbar">
  <div class="search">
    <span class="search-icon" aria-hidden="true"><Icon name="search" size={14} /></span>
    <input
      bind:this={input}
      placeholder="搜索标题…"
      value={search}
      oninput={(event) => onsearch(event.currentTarget.value)}
      spellcheck="false"
    />
    {#if search}
      <button class="link" onclick={onclearsearch}>清除</button>
    {/if}
  </div>

  {#if canSort}
    <button class="chip" class:on={sort === 'added'} onclick={onsort}>
      最近添加 {sort === 'added' ? (direction === 'asc' ? '↑' : '↓') : ''}
    </button>
  {/if}

  <span class="stats">{total > 0 ? `${total} 首` : ''}</span>
  <button class="icon-btn" onclick={onimport} title="导入整理（beets）" aria-label="导入整理">
    <Icon name="folder" size={15} />
  </button>
  <button class="icon-btn" onclick={onsettings} title="设置" aria-label="设置">
    <Icon name="settings" size={15} />
  </button>
  <button class="btn" onclick={onchooseroot} disabled={scanning}>选择目录</button>
  <button class="btn primary" onclick={onscan} disabled={scanning}>
    {scanning ? '扫描中…' : '扫描曲库'}
  </button>
</div>
