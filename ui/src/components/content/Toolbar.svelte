<script lang="ts">
  import type { Snippet } from 'svelte';
  import Chip from '../ui/Chip.svelte';
  import TextTabs from '../ui/TextTabs.svelte';
  import { t } from '../../lib/i18n.svelte';

  /**
   * 页内工具条零件：tab 组、筛选 chip、右侧次级动作。
   *
   * 承担两类入口：曲库的三个内置视图切换（全部曲目 / 最近播放 /
   * 常听），以及设置页的分组跳转 chip。两者都需要一个不占用标题区的导航位，
   * 且只在特定视图存在，因此独立于标题。
   *
   * 与 `Header` 为两个独立零件，不并排、不组合：页面按顺序书写两行标签，
   * 可在其间插入其他内容。两者生命周期不同——标题静态，工具条随视图与空态
   * 出现或消失（歌单页无 tab，空库无筛选 chip）。
   *
   * 三部分均可选，全部为空时整条不渲染（不占高度，不留孤立的分隔线）。
   */

  export interface ToolbarTab {
    id: string;
    label: string;
    /** 可选计数，如「常听 12」 */
    count?: number;
    /** 置灰失效：不可点（beets 未安装时的分析/整理） */
    disabled?: boolean;
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
  <!-- 语义用 nav + aria-current 而不是 role="tablist"/"tab"：设置页的 chip 是「滚动到某个区块」而不是「切换面板」，硬套 tab
  角色会让读屏软件期待一个 tabpanel。两者视觉同构、语义都是"当前项"，aria-current 正好覆盖两种。 -->
  <div class="content-toolbar">
    {#if tabs?.length}
      <TextTabs
        items={tabs}
        value={active ?? ''}
        onchange={(id) => ontab?.(id)}
        label={t('common.viewTabs')}
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

  /* 分隔线只跟 tab 走：tab 行是页面级分区，值得一条线；
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
