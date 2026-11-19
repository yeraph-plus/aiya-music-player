<script lang="ts">
  import type { Snippet } from 'svelte';

  /** 滚动锚点与排序导航使用 nav/current；互斥内容面板使用 TabList 与 Bits Tabs。 */
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
      type="button"
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
