<script lang="ts">
  /**
   * 行内错误条（kit 第 16 件，docs/24 §6 零件表 20）。
   *
   * 「错误分两类」的呈现落点：
   * - **可重试**：给一个动作按钮（如「跳到下一首」「重试」），用户能自救；
   * - **阻断**：只给「关闭」，用户需要去别处处理（扫盘、查 beets）。
   * 区分方式不是颜色或图标，而是**有没有动作可点**——`action` 传了就渲染按钮。
   *
   * `aria-live="assertive"` 只用在这里：错误是真阻断（docs/19 §5），
   * 播放状态之类的 10Hz 更新绝不挂这个属性。
   */
  interface Props {
    message: string;
    ondismiss?: () => void;
    action?: { label: string; onclick: () => void };
  }

  let { message, ondismiss, action }: Props = $props();
</script>

<div class="k-inline-error" role="alert" aria-live="assertive">
  <span class="msg">{message}</span>
  {#if action}
    <button class="link" onclick={action.onclick}>{action.label}</button>
  {/if}
  {#if ondismiss}
    <button class="link" onclick={ondismiss}>关闭</button>
  {/if}
</div>

<style>
  .k-inline-error {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 22px 8px;
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, var(--danger) 35%, transparent);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    font-size: 12px;
  }

  .msg {
    flex: 1;
    min-width: 0;
    word-break: break-word;
  }
</style>
