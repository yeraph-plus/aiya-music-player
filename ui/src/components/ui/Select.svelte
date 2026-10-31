<script lang="ts">
  import { Select } from 'bits-ui';

  /**
   * 下拉选择（kit 零件）：受控的 `value`/`onchange`，选项外置。
   *
   * 为什么不用原生 `<select>`：Chromium 里给 select 设样式**去不掉**原生
   * 控件外观，弹出的选项列表更是完全不受 CSS 管——深色主题下出现一条
   * 浏览器原生亮色下拉条（2026-10-28 用户实测两次）。bits-ui 的 Select
   * 用 portal 渲染选项列表（跟随主题变量），还白送键盘导航、typeahead、
   * 碰撞翻转和 aria 联动。
   *
   * 触发按钮的外观与 `Field` 的输入框同族（k-field-box 同参数）：
   * --control-h 高、--radius-md 圆角、--surface-input 底、focus 描边。
   * 选项列表沿用 `Menu` 的深色面板参数（surface-4 + 毛玻璃 + 10px 圆角）。
   *
   * 选项的 `value` 允许空串（"任意流派"这类"不限"语义项）。
   */

  interface Props {
    /** 触发按钮的 aria 标签（可见标签在旁边的布局里，读屏靠这个） */
    label: string;
    value: string;
    options: Array<{ value: string; label: string }>;
    /** value 不在 options 里时触发按钮显示的占位文案 */
    placeholder?: string;
    disabled?: boolean;
    /** 触发按钮的原生 title（悬停提示；与 Tooltip 二选一，见 Tooltip.svelte 的分工规矩） */
    title?: string;
    onchange: (value: string) => void;
  }

  let { label, value, options, placeholder, disabled = false, title, onchange }: Props = $props();

  const selectedLabel = $derived(
    options.find((o) => o.value === value)?.label ?? placeholder ?? '',
  );
</script>

<Select.Root type="single" {value} {disabled} onValueChange={(v) => onchange(v)}>
  <Select.Trigger class="k-select-trigger" aria-label={label} {title}>
    <span class="k-select-value" class:placeholder={!selectedLabel}>{selectedLabel}</span>
    <svg
      class="k-select-chevron"
      width="10"
      height="6"
      viewBox="0 0 10 6"
      fill="none"
      aria-hidden="true"
    >
      <path d="M1 1l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  </Select.Trigger>
  <Select.Portal>
    <Select.Content class="k-select-content" sideOffset={4} collisionPadding={8}>
      <Select.Viewport class="k-select-viewport">
        {#each options as o (o.value)}
          <Select.Item class="k-select-item" value={o.value} label={o.label}>
            {o.label}
          </Select.Item>
        {/each}
      </Select.Viewport>
    </Select.Content>
  </Select.Portal>
</Select.Root>

<style>
  /*
   * bits-ui 组件的 class 不带 scoped 哈希 → 样式一律 :global(.k-*)。
   * 与 Menu.svelte 同一条规矩（docs/24 §4.5）。
   */
  :global(.k-select-trigger) {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--control-h);
    padding: 0 var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
    color: var(--text-1);
    font-size: var(--fs-base);
    transition:
      border-color var(--dur-fast) var(--ease),
      background var(--dur-fast) var(--ease);
    outline: none;
  }

  :global(.k-select-trigger:focus-visible),
  :global(.k-select-trigger[data-state='open']) {
    border-color: var(--accent);
    background: var(--surface-hover);
  }

  :global(.k-select-value) {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  :global(.k-select-value.placeholder) {
    color: var(--text-3);
  }

  :global(.k-select-chevron) {
    flex: none;
    color: var(--text-2);
    transition: transform var(--dur-fast) var(--ease);
  }

  :global(.k-select-trigger[data-state='open'] .k-select-chevron) {
    transform: rotate(180deg);
  }

  :global(.k-select-content) {
    z-index: var(--z-select);
    /* 与触发按钮同宽（bits-ui 会设 --bits-select-anchor-width），长选项再撑开 */
    width: var(--bits-select-anchor-width, max-content);
    min-width: 120px;
    max-height: 320px;
    padding: 6px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-4);
    box-shadow: var(--shadow-4);
    backdrop-filter: blur(var(--blur-pop));
    outline: none;
  }

  :global(.k-select-viewport) {
    display: flex;
    flex-direction: column;
  }

  :global(.k-select-item) {
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border-radius: 7px;
    font-size: var(--fs-base);
    color: var(--text-1);
    cursor: default;
    user-select: none;
    outline: none;
  }

  /* 键盘导航与 hover 同写 data-highlighted，别只写 :hover */
  :global(.k-select-item[data-highlighted]) {
    background: var(--surface-hover);
  }

  :global(.k-select-item[data-selected]) {
    color: var(--accent-text);
    background: var(--accent-weak);
  }
</style>
