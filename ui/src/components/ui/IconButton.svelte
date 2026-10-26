<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 图标按钮：**没有文字**的按钮，所以 `label` 是必填的 —— 它同时产出
   * `aria-label` 与 `title`（鼠标悬停可见）。没有 label 的图标按钮等于
   * 对读屏软件不存在，这类问题在代码评审里看不出来，只在真机上暴露。
   *
   * 取代 `.icon-btn` / `.icon-btn.on` / `.icon-btn.tiny` / `.icon-btn.play`。
   *
   * `pressed` 与 `active` 是两件事，旧类名 `.on` 把它们混成一个：
   * - `pressed` → `aria-pressed`，用于**切换按钮**（随机播放开/关）
   * - `active`  → 只是视觉高亮，用于**当前项**（队列抽屉开着）
   */

  interface Props {
    /** 必填：同时用于 aria-label 与 title */
    label: string;
    size?: 'sm' | 'md' | 'lg';
    /** 切换按钮的按下态（会写 aria-pressed） */
    pressed?: boolean;
    /** 纯视觉高亮（不写 aria-pressed） */
    active?: boolean;
    disabled?: boolean;
    children: Snippet;
    onclick?: (event: MouseEvent) => void;
    ref?: HTMLButtonElement | null;
  }

  let {
    label,
    size = 'md',
    pressed,
    active = false,
    disabled = false,
    children,
    onclick,
    ref = $bindable(null),
  }: Props = $props();
</script>

<button
  bind:this={ref}
  type="button"
  class="k-icon-btn"
  class:sm={size === 'sm'}
  class:lg={size === 'lg'}
  class:on={active}
  class:pressed={pressed === true}
  aria-label={label}
  title={label}
  aria-pressed={pressed === undefined ? undefined : pressed}
  {disabled}
  {onclick}
>
  {@render children()}
</button>

<style>
  .k-icon-btn {
    display: inline-grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-md);
    color: var(--text-2);
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  .k-icon-btn:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .k-icon-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .k-icon-btn.sm {
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
  }

  .k-icon-btn.lg {
    width: 36px;
    height: 36px;
    border-radius: var(--radius-lg);
  }

  /* 当前项：底色 + 主色图标 */
  .k-icon-btn.on {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  /* 切换按钮的按下态：用主色，和"当前项"区分开 */
  .k-icon-btn.pressed {
    background: var(--accent-weak);
    color: var(--accent-text);
  }
</style>
