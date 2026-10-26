<script lang="ts">
  import { Checkbox } from 'bits-ui';
  import { Check, Minus } from '@lucide/svelte';

  /**
   * 三态选框：**唯一的勾选入口**。
   *
   * 内核是 `bits-ui/Checkbox.Root`（docs/23 的结论，本文不重开）：它负责
   * `role="checkbox"`、`aria-checked="mixed"`、空格键、焦点。我们只负责外观。
   *
   * ## 两条必须遵守的约定（都来自实测）
   *
   * 1. **不暴露 `onCheckedChange`**。它的内部 click 处理器会先翻自己的 state，
   *    调用方拿到回调再翻一次受控值 → 视觉与模型正好反相（docs/23 实测：
   *    勾了的行显示未勾）。勾选态只由 `checked` 决定，**点击处理挂包装层**：
   *
   *    ```svelte
   *    <span tabindex="-1" onclick={toggle}>
   *      <Checkbox {checked} label="选择" />
   *    </span>
   *    ```
   *
   *    包装层的 `tabindex="-1"` 让它可编程聚焦但不进 Tab 序列 —— 真正的键盘入口
   *    是内层 `button`（bits-ui 渲染的）。`-1` 而不是 `0`，因为 `0` 会让 Tab
   *    在每一行停两次。
   * 2. **不要在包装层 `stopPropagation` 阻止行点击**。Svelte 5 把事件委托到根节点
   *    统一派发，同一批 handler 之间 `stopPropagation` 不保证顺序；要让行"认出
   *    点击来源"（`closest('.k-check')`），而不是靠阻止冒泡。
   */

  interface Props {
    checked: boolean;
    indeterminate?: boolean;
    disabled?: boolean;
    /** 无障碍名称，如「选择《曲名》」或「全选」 */
    label: string;
  }

  let { checked, indeterminate = false, disabled = false, label }: Props = $props();
</script>

<Checkbox.Root class="k-check" {checked} {indeterminate} {disabled} aria-label={label}>
  {#if indeterminate}
    <Minus size={11} strokeWidth={3} aria-hidden="true" />
  {:else if checked}
    <Check size={11} strokeWidth={3} aria-hidden="true" />
  {/if}
</Checkbox.Root>

<style>
  :global(.k-check) {
    display: grid;
    place-items: center;
    flex: none;
    width: 15px;
    height: 15px;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    background: var(--surface-input);
    color: var(--play-fg);
    transition:
      background var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }

  :global(.k-check:hover:not(:disabled)) {
    border-color: var(--text-3);
  }

  :global(.k-check[data-state='checked']),
  :global(.k-check[data-state='indeterminate']) {
    background: var(--play-bg);
    border-color: var(--play-bg);
  }

  :global(.k-check:disabled) {
    opacity: 0.35;
    cursor: default;
  }
</style>
