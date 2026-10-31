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
    /** 转圈并禁止点按（发布前 UI 审查 P4：此前播放键 loading 是手写替身） */
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

  /* 按下反馈（发布前 UI 审查 P2-5）：Button/Card 都有 :active，这里补齐 */
  .k-icon-btn:active:not(:disabled) {
    background: var(--surface-active);
  }

  /* 面板开合态：与 DrawerBtn 时代 [aria-expanded='true'] 同视觉（accent 底），
     和 .on（当前项 surface 底）、.pressed（切换态）三分 */
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

  /* 当前项：accent 底（发布前 UI 审查 P2-5 收编——此前 .on 是 surface 底，
     ListToolbar 被迫用 :global 覆盖成 accent；全仓唯一 active 用户是筛选键，
     直接统一成与 pressed 同色系，组件外的样式补丁随之删除） */
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
