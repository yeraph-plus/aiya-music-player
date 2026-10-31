<script lang="ts">
  import { Switch } from 'bits-ui';

  /**
   * 开关：设置页里"开/关"类选项。
   *
   * 内核 `bits-ui/Switch.Root`：`role="switch"`、`aria-checked`、空格/回车切换。
   * 取代全局 `.switch` / `.switch.on`（那两个只有鼠标，键盘按不动）。
   *
   * 与 Checkbox 的区别：它**没有**受控 double-flip 的问题 —— 状态翻转由 bits-ui
   * 内部完成后回调 `onCheckedChange`，调用方只写设置、不回写本地副本。
   * 唯一约束：不要在回调里再手动取反一次（会来回跳）。
   *
   * 可见标签由调用方提供（设置页的 `.setting-title`），`label` 只用于无障碍。
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
