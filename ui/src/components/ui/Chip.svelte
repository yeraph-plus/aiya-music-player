<script lang="ts">
  /**
   * 筛选 / 状态 chip：一个胶囊，可点（作为筛选开关）或可关（右侧 ×）。
   *
   * 取代 `.chip` / `.chip.on` / `.filter` / `.mode-chip` 四份规则，以及设置页
   * 那 18 个同构的锚点按钮。
   *
   * 外层是 `<span>` 而不是 `<button>`：可关的 chip 内部还有一个 × 按钮，
   * button 里套 button 是非法的（浏览器会静默拆开，交互随机失灵）。
   */

  interface Props {
    label: string;
    /** 选中态：主色浅底 */
    on?: boolean;
    /** 有点击行为时，外层退化为不可点容器、内层是可点按钮 */
    onclick?: () => void;
    /** 有关闭行为时渲染右侧 ×（`aria-label` 自动生成） */
    onclear?: () => void;
    disabled?: boolean;
    title?: string;
  }

  let { label, on = false, onclick, onclear, disabled = false, title }: Props = $props();
</script>

<span class="k-chip" class:on class:with-x={!!onclear} {title}>
  {#if onclick}
    <button
      type="button"
      class="k-chip-main"
      aria-pressed={on}
      {disabled}
      onclick={() => onclick?.()}
    >
      {label}
    </button>
  {:else}
    <span class="k-chip-main static">{label}</span>
  {/if}

  {#if onclear}
    <button
      type="button"
      class="k-chip-x"
      aria-label={`清除${label}`}
      title={`清除${label}`}
      onclick={() => onclear?.()}
    >
      ×
    </button>
  {/if}
</span>

<style>
  .k-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 24px;
    max-width: 100%;
    padding: 0 9px;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    background: var(--surface-input);
    color: var(--text-2);
    font-size: var(--fs-sm);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }

  .k-chip.with-x {
    padding-right: 3px;
  }

  .k-chip:hover {
    border-color: var(--line-strong);
    color: var(--text-1);
  }

  .k-chip.on {
    background: var(--accent-weak);
    border-color: transparent;
    color: var(--accent-text);
  }

  .k-chip-main {
    display: inline-flex;
    align-items: center;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: inherit;
    font: inherit;
  }

  .k-chip-main.static {
    padding: 0;
  }

  .k-chip-main:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .k-chip-x {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: var(--text-3);
    font-size: 13px;
    line-height: 1;
  }

  .k-chip-x:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }
</style>
