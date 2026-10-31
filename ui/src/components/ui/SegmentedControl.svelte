<script lang="ts">
  import { RadioGroup } from 'bits-ui';
  import type { Snippet } from 'svelte';

  /**
   * 分段控件：2–5 个互斥短选项（密度、输出模式、缓冲）。
   *
   * 内核 `bits-ui/RadioGroup`，**不是 ToggleGroup**。2026-10-26 实测换的：
   * ToggleGroup（`type="single"`）点/空格按当前项会**取消选中**，回调给
   * `undefined`；这里只吃掉 undefined 不写回，结果 UI 变成"两个都不选"、
   * 设置里却还是旧值 —— 状态与画面分叉。RadioGroup 天然不允许空值，
   * 与"分段控件永远有值"这个语义同构（实测：Space 后仍是 checked 的那个）。
   *
   * RadioGroup 渲染 `role="radiogroup"` + `role="radio"`，`data-state` 是
   * `checked` / `unchecked`（不是 ToggleGroup 的 `on` / `off`）。
   * 方向键走焦点（roving）+ 选中，这是 WAI-ARIA radio group 的标准行为。
   *
   * `orientation="horizontal"` 必须显式给：默认 vertical，左右方向键不生效。
   *
   * 值类型是 `string`，调用方在自己的联合类型里窄化（`onchange` 里断言）。
   */

  interface Props {
    value: string;
    options: ReadonlyArray<{ value: string; label: string; title?: string }>;
    /** 值类型是 string：调用方在自己的联合类型里窄化（`v as ListDensity`）。
     *  不用组件泛型 —— Svelte 的 `generics` 属性在这里推导不出来（数组字面量
     *  先被推成 string[]），最后还是要断言，不如把断言写在调用点。 */
    onchange: (value: string) => void;
    /** 无障碍名称（这一组选的是什么） */
    label: string;
    disabled?: boolean;
    /** 紧凑档：设置页行内用默认，工具栏里可以用 sm */
    size?: 'sm' | 'md';
    /**
     * 每项的前置图标，入参是该项的 value。
     * 主题切换要月/日图标，但图标**不能**写进 `options`：
     * options 是纯数据（`ReadonlyArray<{value,label,title}>`），塞 Snippet 进去
     * 会让调用方每次渲染都新建片段；按 value 回查也更直白。
     */
    icon?: Snippet<[string]>;
  }

  let { value, options, onchange, label, disabled = false, size = 'md', icon }: Props = $props();
</script>

<RadioGroup.Root
  {value}
  aria-label={label}
  {disabled}
  orientation="horizontal"
  onValueChange={(next) => onchange(next)}
  class={`k-seg${size === 'sm' ? ' sm' : ''}`}
>
  {#each options as opt (opt.value)}
    <RadioGroup.Item value={opt.value} class="k-seg-item" title={opt.title} disabled={disabled}>
      {#if icon}<span class="k-seg-ic">{@render icon(opt.value)}</span>{/if}
      {opt.label}
    </RadioGroup.Item>
  {/each}
</RadioGroup.Root>

<style>
  :global(.k-seg) {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
  }

  :global(.k-seg-item) {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: calc(var(--control-h) - 6px);
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    color: var(--text-2);
    font-size: var(--fs-sm);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  :global(.k-seg.sm .k-seg-item) {
    height: 22px;
    padding: 0 7px;
    font-size: var(--fs-xs);
  }

  :global(.k-seg-item:hover:not([disabled])) {
    color: var(--text-1);
  }

  :global(.k-seg-item[data-state='checked']) {
    background: var(--surface-3);
    color: var(--text-1);
    box-shadow: var(--shadow-1);
  }

  :global(.k-seg-item[disabled]) {
    opacity: 0.4;
    cursor: default;
  }

  :global(.k-seg-ic) {
    display: inline-flex;
    align-items: center;
  }
</style>
