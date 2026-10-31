<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 按钮：**唯一**的按钮外观定义。
   *
   * 已取代并删除全局 `.btn` / `.btn.primary` / `.btn.danger` / `.apply-btn`
   * 四份手写规则（原在 `styles/shell.css` 与 `ImportPanel` 里，各写了一遍
   * height/padding/border-radius）。B3b-3 收尾后 `grep 'class="btn'` 在
   * `ui/src` 已归零，旧规则随之删除（2026-10-26）。
   *
   * 为什么不用 `bits-ui/button`：`Button.Root` 是 builder 模式，产出的是裸 `<button>`
   * + `data-state`，样式仍然全要自己写；而按钮的难点恰恰在样式变体矩阵，不在行为
   * （按下/禁用是浏览器原生）。判断线见 docs/24 §4.5。
   *
   * 类名用 `k-` 前缀而不是 `.btn`：替换期与旧全局 CSS **共存**，`.btn` 两边同名
   * 会让"这条规则到底谁赢"取决于 scoped 哈希，调试时看不出来。
   */

  interface Props {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'link';
    /** sm=28px（表格行内、chip 旁），md=--control-h（默认 32px） */
    size?: 'sm' | 'md';
    disabled?: boolean;
    /** 转圈并禁止点按；正文保留（避免按钮宽度在加载时跳动） */
    loading?: boolean;
    title?: string;
    /** 只有图标、没有文字时必填（同时补 `aria-label`） */
    ariaLabel?: string;
    /** 前置图标，通常 `{#snippet icon()}<Play size={13} />{/snippet}` */
    icon?: Snippet;
    children?: Snippet;
    onclick?: (event: MouseEvent) => void;
    /** 元素引用：对话框拿它把初始焦点放到「取消」上（组件没有别的办法拿到内层 DOM） */
    ref?: HTMLButtonElement | null;
  }

  let {
    variant = 'secondary',
    size = 'md',
    disabled = false,
    loading = false,
    title,
    ariaLabel,
    icon,
    children,
    onclick,
    ref = $bindable(null),
  }: Props = $props();
</script>

<button
  bind:this={ref}
  type="button"
  class="k-btn"
  class:primary={variant === 'primary'}
  class:ghost={variant === 'ghost'}
  class:danger={variant === 'danger'}
  class:link={variant === 'link'}
  class:sm={size === 'sm'}
  class:loading
  disabled={disabled || loading}
  aria-busy={loading}
  {title}
  aria-label={ariaLabel}
  {onclick}
>
  {#if loading}
    <span class="k-spin" aria-hidden="true"></span>
  {:else if icon}
    <span class="k-btn-icon" aria-hidden="true">{@render icon()}</span>
  {/if}
  {#if children}<span class="k-btn-label">{@render children()}</span>{/if}
</button>

<style>
  .k-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: var(--control-h);
    padding: 0 var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
    color: var(--text-1);
    font-size: var(--fs-base);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  .k-btn:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  .k-btn:active:not(:disabled) {
    background: var(--surface-active);
  }

  .k-btn.sm {
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--fs-sm);
    border-radius: var(--radius-sm);
  }

  .k-btn.primary {
    background: var(--accent-weak);
    border-color: var(--accent-weak);
    color: var(--accent-text);
  }

  .k-btn.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 26%, transparent);
    border-color: color-mix(in srgb, var(--accent) 34%, transparent);
  }

  .k-btn.ghost {
    background: transparent;
    border-color: transparent;
    color: var(--text-2);
  }

  .k-btn.ghost:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: transparent;
    color: var(--text-1);
  }

  .k-btn.danger {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 32%, transparent);
  }

  .k-btn.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    border-color: color-mix(in srgb, var(--danger) 48%, transparent);
  }

  /*
   * 链接按钮（发布前 UI 审查 P3）：取代散落 6 个文件的 .link / .rate-reset /
   * .rule-del 手写族——同一意图（行内 accent 链接状动作）此前 5 种类名各写
   * 一遍，且全部没有焦点环。形状上剥掉按钮盒（高/内距/边框/底色归零），
   * 键盘焦点环来自 base.css 的全局 :focus-visible。
   */
  .k-btn.link {
    height: auto;
    padding: 0 2px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--accent-text);
    font-size: inherit;
    line-height: inherit;
    white-space: normal;
  }

  .k-btn.link:hover:not(:disabled) {
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .k-btn.link.sm {
    height: auto;
    padding: 0 2px;
  }

  .k-btn:disabled {
    opacity: var(--disabled-opacity);
    cursor: default;
  }

  .k-btn.loading {
    cursor: progress;
  }

  .k-btn-icon,
  .k-spin {
    flex: none;
    display: grid;
    place-items: center;
  }

  /* 转圈用 conic-gradient 而不是 SVG：不需要额外 DOM，颜色跟着 currentColor */
  .k-spin {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: conic-gradient(transparent 0 25%, currentColor 75% 100%);
    mask: radial-gradient(circle, transparent 0 3px, #000 3.5px);
    animation: k-spin 720ms linear infinite;
  }

  @keyframes k-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .k-spin {
      animation-duration: 2s;
    }
  }
</style>
