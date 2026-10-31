<script lang="ts">
  import { fade } from 'svelte/transition';

  /**
   * 轻提示（kit 第 17 件，docs/24 §6 零件表 20）。
   *
   * 只管呈现：出现/消失的时机归 `stores/notify`（2.6s 自动消失），
   * 这里不做计时——两处职责分开，别的宿主（比如将来的设置页内提示）
   * 也能复用同一段样式。
   *
   * `aria-live="polite"`（docs/19 §5）：成功反馈不打断朗读；
   * assertive 是 `InlineError` 的专属属性。
   *
   * 进场走 CSS `toast-in`，退场补 `out:fade`（发布前 UI 审查 P1-3：
   * 此前 2.6s 到点瞬消，与进场不成对）。只退场用 svelte 过渡、进场留给
   * CSS：svelte 的 `transition:` 会同时接管进出，反而覆盖掉 toast-in。
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
    /* 65：排在 Menu(61)/Select(62) 之上、Tooltip(70) 之下——此前与两者同值 60，
       菜单开着时弹 toast 的层叠结果取决于 portal 的 DOM 顺序（发布前 UI 审查 P10） */
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
