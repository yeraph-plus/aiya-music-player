<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 页内工具条零件：tab 组 + 筛选 chip + 右侧次级动作。
   *
   * 2026-10-26 壳体收口从两个地方合并出来（docs/24 §3.1 修订 v2）：
   *
   * - **曲库**原本没有任何视图切换入口 —— 三个内置视图（全部曲目 / 最近播放 /
   *   常听）只能从首页「查看全部」进，进去之后回不去。
   * - **设置**原本把 18 个锚点 chip 塞在标题 header 的右侧，`.chip.on` 的
   *   active 只能靠滚动反查写回，标题区在兼职承载导航状态。
   *
   * 与 `Header` 的关系：**两个零件，不在一行、不组合**。页面写两行标签，
   * 中间想插扫描进度条就插。它们生命周期不同 —— 标题是静态的，工具条会因
   * 视图 / 空态出现或消失（歌单页没有 tab，空库没有筛选 chip）。
   *
   * 三处都是可选的，**全空时整条不渲染**（不占高度，不留一条孤立的分隔线）。
   */

  export interface ToolbarTab {
    id: string;
    label: string;
    /** 可选计数，如「常听 12」 */
    count?: number;
  }

  interface Props {
    tabs?: ToolbarTab[];
    /** 当前选中的 tab id；只影响高亮，不参与 tab 内部状态 */
    active?: string | null;
    ontab?: (id: string) => void;
    /** 已应用的筛选，右侧带一个 × */
    filters?: Array<{ label: string; onclear: () => void }>;
    /** 工具条最右的次级动作 */
    end?: Snippet;
  }

  let { tabs, active, ontab, filters, end }: Props = $props();
</script>

{#if tabs?.length || filters?.length || end}
  <!--
    语义用 nav + aria-current 而不是 role="tablist"/"tab"：设置页的 chip 是
    「滚动到某个区块」而不是「切换面板」，硬套 tab 角色会让读屏软件期待一个
    tabpanel。两者视觉同构、语义都是"当前项"，aria-current 正好覆盖两种。
  -->
  <div class="content-toolbar">
    {#if tabs?.length}
      <nav class="pt-tabs" aria-label="视图切换">
        {#each tabs as tab (tab.id)}
          <button
            class="pt-tab"
            class:active={active === tab.id}
            aria-current={active === tab.id ? 'true' : undefined}
            onclick={() => ontab?.(tab.id)}
          >
            {tab.label}
            {#if tab.count !== undefined}<span class="pt-count">{tab.count}</span>{/if}
          </button>
        {/each}
      </nav>
    {/if}

    {#each filters ?? [] as filter (filter.label)}
      <span class="pt-chip">
        {filter.label}
        <button
          class="pt-chip-x"
          aria-label={`清除${filter.label}`}
          title={`清除${filter.label}`}
          onclick={filter.onclear}
        >
          ×
        </button>
      </span>
    {/each}

    <div class="pt-spacer"></div>

    {#if end}<div class="pt-end">{@render end()}</div>{/if}
  </div>
{/if}

<style>
  .content-toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 0 10px;
    border-bottom: 1px solid var(--line);
    min-width: 0;
  }

  .pt-tabs {
    display: flex;
    gap: 2px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .pt-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    padding: 5px 11px;
    border-radius: 8px;
    color: var(--text-2);
    font-size: 12px;
    white-space: nowrap;
  }

  .pt-tab:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .pt-tab.active {
    background: var(--surface-active);
    color: var(--text-1);
  }

  .pt-count {
    color: var(--text-3);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }

  .pt-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px 3px 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface-input);
    color: var(--text-2);
    font-size: 11.5px;
    white-space: nowrap;
  }

  .pt-chip-x {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    color: var(--text-3);
    font-size: 13px;
    line-height: 1;
  }

  .pt-chip-x:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .pt-spacer {
    flex: 1;
    min-width: 6px;
  }

  .pt-end {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
