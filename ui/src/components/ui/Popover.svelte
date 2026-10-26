<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Popover } from 'bits-ui';

  /**
   * 弹出面板：触发元素 + 内容。
   *
   * 取代 `PlayerBar` 里手写的那套（`svelte:window onpointerdown` 判 `box.contains`
   * 收面板 + `onkeydown` 判 Esc）：手写版只有"点外面关闭"，没有
   * **Esc 后把焦点还回触发按钮**、没有**碰撞翻转**（面板在窗口下沿会被裁掉）、
   * 也没有 aria-expanded / aria-controls 的联动。
   *
   * ### 为什么 trigger 是 snippet 而不是 `children`
   *
   * 触发元素是**调用方的按钮**（播放条上的倍速 chip），bits-ui 需要把
   * `aria-expanded` / `aria-controls` / `onclick` / `onkeydown` 挂到它身上。
   * 所以 `trigger` 拿到 props 后自己展开到元素上 —— 这也是唯一能让组件样式
   * 留在调用方手里的写法（`Popover.Trigger` 默认会渲染自己的 button）。
   *
   * 类型用 `Record<string, any>`：这些 props 是 bits-ui 内部的合并结果，
   * 没有导出成公共类型；写成 `unknown` 会让调用方的 `{...props}` 在
   * svelte-check 里报"不能展开到 button 属性"。
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
    z-index: 55;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-4);
    box-shadow: 0 18px 40px var(--shadow);
    backdrop-filter: blur(18px);
    outline: none;
  }
</style>
