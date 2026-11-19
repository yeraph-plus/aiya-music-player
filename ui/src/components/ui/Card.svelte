<script lang="ts">
  import type { Snippet } from 'svelte';
  import Cover from './Cover.svelte';

  /**
   * 卡片：首页歌单与曲库入口的方块，尺寸与悬停反馈由此统一。
   *
   * 入口卡片渲染为 button；带独立操作的卡片渲染为 div，避免嵌套交互元素。
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
    avatar?: boolean;
    actions?: Snippet;
    metadata?: Snippet;
    children?: Snippet;
  }

  let { title, subtitle, cover, icon, onclick, titleAttr, avatar = false, actions, metadata, children }: Props = $props();
</script>

{#snippet content()}
  {#if cover !== undefined}
    <Cover src={cover} size="md" />
  {:else if icon}
    <span class="k-card-icon" class:avatar aria-hidden="true">{@render icon()}</span>
  {/if}
  <span class="k-card-text">
    <span class="k-card-title">{title}</span>
    {#if metadata}<span class="k-card-sub">{@render metadata()}</span>
    {:else if subtitle}<span class="k-card-sub">{subtitle}</span>{/if}
  </span>
{/snippet}

{#if children}
  <div class="k-card with-body">
    <div class="k-card-head">{@render content()}{#if actions}<div class="k-card-actions">{@render actions()}</div>{/if}</div>
    <div class="k-card-body">{@render children()}</div>
  </div>
{:else if actions}
  <div class="k-card with-actions">
    {@render content()}
    <div class="k-card-actions">{@render actions()}</div>
  </div>
{:else}
  <button type="button" class="k-card" title={titleAttr ?? title} {onclick}>
    {@render content()}
  </button>
{/if}

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

  button.k-card:hover {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  button.k-card:active {
    transform: scale(0.995);
  }

  .k-card-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 3rem;
    height: 3rem;
    border-radius: var(--radius-md);
    background: var(--surface-cover);
    color: var(--accent-text);
  }

  .k-card-actions {display:flex;align-items:center;gap:var(--space-2);flex:none;}
  .k-card-icon.avatar {border-radius:var(--radius-pill);}
  .with-body {display:block;padding:var(--space-5);}.k-card-head {display:flex;align-items:center;gap:var(--space-3);}.k-card-body {border-top:1px solid var(--line);margin-top:var(--space-3);padding-top:var(--space-4);}

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
    font-size: var(--fs-md);
    font-weight: 500;
  }

  .k-card-sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-base);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
</style>
