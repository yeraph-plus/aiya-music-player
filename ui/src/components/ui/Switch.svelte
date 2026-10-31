<script lang="ts">
  import { Switch } from 'bits-ui';

  /**
   * 开关：设置页中"开/关"类选项的唯一实现。
   *
   * 内核为 `bits-ui/Switch.Root`，提供 `role="switch"`、`aria-checked` 与空格/回车
   * 切换，键盘可达（手写实现通常只有鼠标响应）。
   *
   * 与 Checkbox 的差别：不存在受控翻转两次的问题。状态翻转由 bits-ui 内部完成后
   * 回调 `onCheckedChange`，调用方只写设置，不回写本地副本。约束是不要在回调内
   * 再手动取反一次，否则值会来回跳变。
   *
   * 可见标签由调用方提供（设置页的 `.setting-title`），`label` 仅用于无障碍。
   */

  interface Props {
    checked: boolean;
    onchange: (checked: boolean) => void;
    /** 无障碍名称；视觉标签由调用方渲染 */
    label: string;
    disabled?: boolean;
  }

  let { checked, onchange, label, disabled = false }: Props = $props();
</script>

<Switch.Root
  {checked}
  {disabled}
  aria-label={label}
  onCheckedChange={(next) => onchange(next)}
  class="k-switch"
>
  <Switch.Thumb class="k-switch-thumb" />
</Switch.Root>

<style>
  :global(.k-switch) {
    position: relative;
    flex: none;
    width: 38px;
    height: 22px;
    padding: 0;
    border-radius: var(--radius-pill);
    background: var(--surface-hover);
    transition: background var(--dur-fast) var(--ease);
  }

  :global(.k-switch[data-state='checked']) {
    background: var(--accent);
  }

  :global(.k-switch:disabled) {
    opacity: var(--disabled-opacity);
    cursor: default;
  }

  :global(.k-switch:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :global(.k-switch-thumb) {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--accent-fg);
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease);
  }

  :global(.k-switch[data-state='checked'] .k-switch-thumb) {
    transform: translateX(16px);
  }
</style>
