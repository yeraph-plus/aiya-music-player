<script lang="ts">
  import type { Snippet } from 'svelte';
  import Chip from '../ui/Chip.svelte';
  import TextTabs from '../ui/TextTabs.svelte';

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
      <TextTabs
        items={tabs}
        value={active ?? ''}
        onchange={(id) => ontab?.(id)}
        label="视图切换"
        variant="pill"
      />
    {/if}

    {#each filters ?? [] as filter (filter.label)}
      <Chip label={filter.label} onclear={filter.onclear} />
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
    min-width: 0;
  }

  /* 分隔线只跟 tab 走（docs/28）：tab 行是页面级分区，值得一条线；
     chips/次级动作的残行不值得——曲库页撤掉 tab 后只剩时间筛选 chip，
     不该再拖一条孤立的分隔线 */
  .content-toolbar:has(:global(.text-tabs)) {
    border-bottom: 1px solid var(--line);
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
