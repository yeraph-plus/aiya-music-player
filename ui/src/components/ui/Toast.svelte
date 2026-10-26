<script lang="ts">
  /**
   * 轻提示（kit 第 17 件，docs/24 §6 零件表 20）。
   *
   * 只管呈现：出现/消失的时机归 `stores/notify`（2.6s 自动消失），
   * 这里不做计时——两处职责分开，别的宿主（比如将来的设置页内提示）
   * 也能复用同一段样式。
   *
   * `aria-live="polite"`（docs/19 §5）：成功反馈不打断朗读；
   * assertive 是 `InlineError` 的专属属性。
   */
  interface Props {
    message: string;
  }

  let { message }: Props = $props();
</script>

<div class="k-toast" aria-live="polite">{message}</div>

<style>
  .k-toast {
    position: fixed;
    left: 50%;
    bottom: calc(var(--player-h) + 16px);
    transform: translateX(-50%);
    z-index: 60;
    padding: 8px 14px;
    border-radius: 999px;
    border: 1px solid var(--line-strong);
    background: var(--surface-4);
    box-shadow: 0 12px 30px var(--shadow);
    font-size: 12px;
    color: var(--text-1);
    animation: toast-in 160ms ease-out;
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
