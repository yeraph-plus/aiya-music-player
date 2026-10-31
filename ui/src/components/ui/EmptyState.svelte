<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 空态：四段式（图标、标题、说明、动作），全部空态的唯一实现，
   * 各页面不再自行组织居中布局与次要文字颜色。
   *
   * `action` 使用 snippet 而非 `label + onclick`：空态中可能出现两个按钮
   * （如"扫描曲库"与"打开设置"），固定字段会迫使其增加第二个 prop。
   */

  interface Props {
    title: string;
    hint?: string;
    /** 图标，通常 22–28px 的 lucide 组件 */
    icon?: Snippet;
    action?: Snippet;
    /** 撑满父容器垂直居中（列表空态用），默认只占自然高度 */
    fill?: boolean;
  }

  let { title, hint, icon, action, fill = false }: Props = $props();
</script>

<div class="k-empty" class:fill>
  {#if icon}<div class="k-empty-icon" aria-hidden="true">{@render icon()}</div>{/if}
  <p class="k-empty-title">{title}</p>
  {#if hint}<p class="k-empty-hint">{hint}</p>{/if}
  {#if action}<div class="k-empty-action">{@render action()}</div>{/if}
</div>

<style>
  .k-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: 36px 22px;
    text-align: center;
    color: var(--text-2);
  }

  .k-empty.fill {
    flex: 1;
    min-height: 0;
  }

  .k-empty-icon {
    color: var(--text-3);
    margin-bottom: var(--space-1);
  }

  .k-empty-title {
    margin: 0;
    font-size: var(--fs-md);
    color: var(--text-1);
  }

  .k-empty-hint {
    margin: 0;
    max-width: 42ch;
    font-size: var(--fs-sm);
    line-height: 1.7;
    color: var(--text-2);
  }

  .k-empty-action {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
