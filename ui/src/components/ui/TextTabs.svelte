<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 文本页签（发布前 UI 审查 P8 抽件）：此前同一交互有**三套实现、两种语义**
   * —— Toolbar 胶囊底（nav+aria-current）、SettingsPanel 下划线（nav，样式与
   * LibraryView 逐行复制）、LibraryView 下划线却套了完整 tablist/tab 角色
   * （又没有 tabpanel，读屏模型失实）。抽成一件后：
   *
   * · 语义只有一种：`nav` + `aria-current="true"`（页签切换的是"当前视图"
   *   而不是 tabpanel 的显隐，tab 角色模型在这里不成立——Toolbar 与
   *   SettingsPanel 的既有注释论证过，LibraryView 归队）。
   * · 外观两档：`pill`（内容工具条）/ `underline`（页面级分组）。
 * · `disabled` 置灰失效（beets 未安装时的分析/整理 tab）：不可点，但位置还在
 *   ——用户看得见"还有这两页"，只是现在进不去。
   * · `count` 是可选角标（工具条的曲目数）；`title` 透传（曲库 tab 的
   *   "再点一次切换顺序"提示）。
   */
  interface TabItem {
    id: string;
    label: string;
    count?: number;
    title?: string;
    /** 置灰失效：不可点（beets 未安装时的分析/整理） */
    disabled?: boolean;
  }

  interface Props {
    items: TabItem[];
    value: string;
    onchange: (id: string) => void;
    /** 无障碍名称（nav 的 aria-label，必填：页签组必须有自己的名字） */
    label: string;
    variant?: 'pill' | 'underline';
    /** 每项标签后的附加内容（曲库 tab 的排序方向箭头）；收当前项做参数 */
    trailing?: Snippet<[TabItem]>;
  }

  let { items, value, onchange, label, variant = 'underline', trailing }: Props = $props();
</script>

<nav class={`text-tabs ${variant}`} aria-label={label}>
  {#each items as tab (tab.id)}
    <button
      class="text-tab"
      class:on={value === tab.id}
      aria-current={value === tab.id ? 'true' : undefined}
      title={tab.title}
      disabled={tab.disabled}
      onclick={() => onchange(tab.id)}
    >
      {tab.label}
      {#if tab.count !== undefined}<span class="text-tab-count">{tab.count}</span>{/if}
      {#if trailing}{@render trailing(tab)}{/if}
    </button>
  {/each}
</nav>

<style>
  .text-tabs {
    display: flex;
    min-width: 0;
  }

  .text-tabs.pill {
    gap: 2px;
    flex-wrap: wrap;
  }

  .text-tabs.underline {
    align-items: baseline;
    gap: 14px;
  }

  .text-tab {
    color: var(--text-2);
    font-size: var(--fs-base);
    white-space: nowrap;
  }

  .text-tab:hover {
    color: var(--text-1);
  }

  .text-tab.on {
    color: var(--text-1);
  }

  /* ---- pill（内容工具条）---- */
  .pill .text-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    padding: 5px 11px;
    border-radius: var(--radius-md);
  }

  .pill .text-tab:hover {
    background: var(--surface-hover);
  }

  .pill .text-tab.on {
    background: var(--surface-active);
  }

  /* ---- underline（页面级分组）---- */
  .underline .text-tab {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 0;
    border-bottom: 2px solid transparent;
  }

  .underline .text-tab.on {
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  .text-tab-count {
    color: var(--text-3);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }

  /* 置灰失效：压暗 + 不响应 hover 抬色（disabled 按钮不触发 click，但 :hover 仍会算） */
  .text-tab:disabled {
    opacity: var(--disabled-opacity);
    cursor: default;
  }

  .text-tab:disabled:hover {
    color: var(--text-2);
    background: transparent;
  }
</style>
