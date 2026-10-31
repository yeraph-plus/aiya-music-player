<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Popover } from 'bits-ui';

  /**
   * 弹出面板：触发元素加内容。
   *
   * 由 bits-ui 承担外部点击关闭、Esc 关闭并将焦点归还触发元素、碰撞翻转
   * （面板在窗口下沿会被裁切）与 aria-expanded / aria-controls 联动。
   *
   * ### trigger 使用 snippet 而非 `children`
   *
   * 触发元素是调用方的按钮（播放条上的倍速 chip），bits-ui 需将 `aria-expanded`、
   * `aria-controls`、`onclick` 与 `onkeydown` 挂到该元素上。`trigger` 取得 props 后
   * 自行展开到元素，这也是让组件样式留在调用方的唯一写法
   * （`Popover.Trigger` 默认渲染自己的 button）。
   *
   * 类型写作 `Record<string, any>`：这些 props 是 bits-ui 内部的合并结果，未导出为
   * 公共类型；写作 `unknown` 会使调用方的 `{...props}` 在 svelte-check 中报无法
   * 展开到 button 属性。
   */

  type TriggerProps = Record<string, any>;

  interface Props {
    open: boolean;
    onopenchange: (open: boolean) => void;
    /** 面板相对触发点的方向，默认向上（贴底部的控件都长这样） */
    side?: 'top' | 'bottom' | 'left' | 'right';
    align?: 'start' | 'center' | 'end';
    sideOffset?: number;
    trigger: Snippet<[TriggerProps]>;
    children: Snippet;
  }

  let {
    open,
    onopenchange,
    side = 'top',
    align = 'center',
    sideOffset = 8,
    trigger,
    children,
  }: Props = $props();
</script>

<Popover.Root {open} onOpenChange={onopenchange}>
  <Popover.Trigger>
    {#snippet child({ props })}
      {@render trigger(props)}
    {/snippet}
  </Popover.Trigger>

  <Popover.Portal>
    <Popover.Content
      class="k-pop"
      {side}
      {align}
      {sideOffset}
      collisionPadding={8}
    >
      {@render children()}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  :global(.k-pop) {
    z-index: var(--z-pop);
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-4);
    box-shadow: var(--shadow-4);
    backdrop-filter: blur(var(--blur-pop));
    outline: none;
  }
</style>
