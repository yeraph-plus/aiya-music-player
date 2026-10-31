<script lang="ts">
  import type { Snippet } from 'svelte';
  import Cover from './Cover.svelte';

  /**
   * 卡片：首页歌单与曲库入口的方块，尺寸与悬停反馈由此统一。
   *
   * 恒渲染为 `<button>`：卡片在本应用中只作为可点击入口存在，不承载交互子元素
   * （HTML 不允许 button 嵌套 button）。需要按钮的卡片应使用 `StatBlock` 加外层
   * 容器组合。
   *
   * `cover` 三态与 `Header` 一致：缺省为无封面位（纯图标）、`null` 为占位符、
   * 字符串为图片 URL。
   */

  interface Props {
    title: string;
    subtitle?: string;
    /** 缺省 = 不显示封面；null = 占位符；字符串 = 图片 URL */
    cover?: string | null;
    /** 无封面时的图标（通常 20px 的 lucide 组件） */
    icon?: Snippet;
    onclick?: () => void;
    titleAttr?: string;
  }

  let { title, subtitle, cover, icon, onclick, titleAttr }: Props = $props();
</script>

<button type="button" class="k-card" title={titleAttr ?? title} {onclick}>
  {#if cover !== undefined}
    <Cover src={cover} size="md" />
  {:else if icon}
    <span class="k-card-icon" aria-hidden="true">{@render icon()}</span>
  {/if}
  <span class="k-card-text">
    <span class="k-card-title">{title}</span>
    {#if subtitle}<span class="k-card-sub">{subtitle}</span>{/if}
  </span>
</button>

<style>
  .k-card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-width: 0;
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    color: var(--text-1);
    text-align: left;
    transition:
      background var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease),
      transform var(--dur-fast) var(--ease);
  }

  .k-card:hover {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  .k-card:active {
    transform: scale(0.995);
  }

  .k-card-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-md);
    background: var(--surface-cover);
    color: var(--accent-text);
  }

  .k-card-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .k-card-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-base);
    font-weight: 500;
  }

  .k-card-sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
</style>
