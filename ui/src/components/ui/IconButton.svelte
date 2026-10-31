<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 图标按钮：无文字，因此 `label` 为必填项，同时产出 `aria-label` 与 `title`
   * （悬停可见）。缺少 label 的图标按钮对读屏软件不可见，此类问题在代码评审中
   * 难以发现，只在真机上暴露。
   *
   * 类名统一为 `.k-icon-btn`，不再区分按语义命名的多套前缀。
   *
   * `pressed` 与 `active` 表示两种状态：
   * - `pressed` → `aria-pressed`，用于切换按钮（如随机播放开关）
   * - `active`  → 仅视觉高亮，用于当前项（如队列抽屉已打开）
   */

  interface Props {
    /** 必填：同时用于 aria-label 与 title */
    label: string;
    size?: 'sm' | 'md' | 'lg';
    /**
     * 外观变体，只管颜色。`solid` 是播放条那颗唯一的实心键（`--play-bg/--play-fg`）——
     * 不给它做 `Button` 的 primary，是因为形状（圆）、尺寸（36）和一屏只有一颗
     * 这两点都不属于通用按钮矩阵。
     */
    variant?: 'ghost' | 'solid';
    /** 圆形：播放条的传输键是圆的，侧栏/对话框的是圆角方 */
    round?: boolean;
    /** 切换按钮的按下态（会写 aria-pressed） */
    pressed?: boolean;
    /** 纯视觉高亮（不写 aria-pressed） */
    active?: boolean;
    /** 面板开合态（写 aria-expanded，样式与 pressed 同级：抽屉触发键用） */
    expanded?: boolean;
    /** 转圈并禁止点按（播放键等待引擎响应时使用） */
    loading?: boolean;
    /** 右下角"生效中"打点（EQ 启用提示；不用 accent 底——那是"抽屉开着"） */
    dot?: boolean;
    disabled?: boolean;
    children: Snippet;
    onclick?: (event: MouseEvent) => void;
    ref?: HTMLButtonElement | null;
  }

  let {
    label,
    size = 'md',
    variant = 'ghost',
    round = false,
    pressed,
    active = false,
    expanded,
    loading = false,
    dot = false,
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
  class:solid={variant === 'solid'}
  class:round
  class:on={active}
  class:pressed={pressed === true}
  aria-label={label}
  title={label}
  aria-pressed={pressed === undefined ? undefined : pressed}
  aria-expanded={expanded}
  aria-busy={loading || undefined}
  disabled={disabled || loading}
  {onclick}
>
  {#if loading}
    <span class="k-icon-spin" aria-hidden="true"></span>
  {:else}
    {@render children()}
  {/if}
  {#if dot}<span class="k-icon-dot" aria-hidden="true"></span>{/if}
</button>

<style>
  .k-icon-btn {
    position: relative; /* dot 的定位锚 */
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

  /* 按下反馈：与 Button、Card 的 :active 行为保持一致 */
  .k-icon-btn:active:not(:disabled) {
    background: var(--surface-active);
  }

  /* 面板开合态（`aria-expanded="true"`）：accent 底，与 `.pressed`（切换态）、
     `.on`（当前项）三者语义不同，取值由各自的属性或类名决定 */
  .k-icon-btn[aria-expanded='true'] {
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  /* 生效中打点：accent 小点，右下角（原 .drawer-dot） */
  .k-icon-dot {
    position: absolute;
    right: 5px;
    bottom: 5px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }

  /* loading 转圈：与 Button 的 k-spin 同款（conic + currentColor，RM 降速）。
     实心播放键上 currentColor = play-fg（深色），白底深圈，与旧手写版一致 */
  .k-icon-spin {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: conic-gradient(transparent 0 25%, currentColor 75% 100%);
    mask: radial-gradient(circle, transparent 0 3px, #000 3.5px);
    animation: k-icon-spin 720ms linear infinite;
  }

  @keyframes k-icon-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .k-icon-spin {
      animation-duration: 2s;
    }
  }

  .k-icon-btn:disabled {
    opacity: var(--disabled-opacity);
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

  /* 当前项：accent 底，表示当前所处的项（如筛选键），与 `.pressed` 的切换语义
     区分开 —— 前者描述位置，后者描述开关状态 */
  .k-icon-btn.on {
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  /* 切换按钮的按下态：用主色，和"当前项"区分开 */
  .k-icon-btn.pressed {
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  .k-icon-btn.round {
    border-radius: var(--radius-pill);
  }

  /* 实心键（播放/暂停）：hover 只提亮，不换底色 —— 换了会闪一下像变灰 */
  .k-icon-btn.solid {
    background: var(--play-bg);
    color: var(--play-fg);
  }

  .k-icon-btn.solid:hover:not(:disabled) {
    background: var(--play-bg);
    color: var(--play-fg);
    filter: brightness(1.06);
  }
</style>
