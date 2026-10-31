<script lang="ts">
  import { fade } from 'svelte/transition';

  /**
   * 轻提示（kit 第 17 件，docs/24 §6 零件表 20）。
   *
   * 只负责呈现：出现与消失的时机归 `stores/notify`（2.6s 自动消失），本组件不做
   * 计时。职责分离后，其他宿主（如页内提示）可复用同一段样式。
   *
   * `aria-live="polite"`（docs/19 §5）：成功反馈不打断朗读；assertive 为
   * `InlineError` 专用。
   *
   * 进场使用 CSS `toast-in`，退场使用 `out:fade`，两者时长匹配。退场使用 svelte
   * 过渡而进场留给 CSS，是因为 `transition:` 会同时接管进入与退出，覆盖 toast-in。
   */
  interface Props {
    message: string;
  }

  let { message }: Props = $props();
</script>

<div class="k-toast" aria-live="polite" out:fade={{ duration: 120 }}>{message}</div>

<style>
  .k-toast {
    position: fixed;
    left: 50%;
    bottom: calc(var(--player-h) + 16px);
    transform: translateX(-50%);
    /* 层序 65：位于 Menu(61)、Select(62) 之上，Tooltip(70) 之下。与菜单同值时，
       菜单打开状态下弹出 toast 的叠放结果取决于 portal 的 DOM 顺序 */
    z-index: var(--z-toast);
    padding: 8px 14px;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--surface-4);
    box-shadow: var(--shadow-3);
    font-size: var(--fs-base);
    color: var(--text-1);
    animation: toast-in 160ms ease-out;
  }

  @media (prefers-reduced-motion: reduce) {
    .k-toast {
      animation: none;
    }
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .k-toast {
      animation: none;
    }
  }
</style>
