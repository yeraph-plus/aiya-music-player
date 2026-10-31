<script lang="ts">
  import { DropdownMenu, Portal } from 'bits-ui';
  import { Check } from '@lucide/svelte';
  import type { MenuItem } from '../../lib/menu';

  /**
   * 右键菜单（数据驱动）：`items` 进，菜单出。
   *
   * 取代 `components/ContextMenu.svelte` —— 那份实现自己算翻转（`Math.min` 夹到
   * 窗口内）、自己铺一层 backdrop 接外点、自己在 `svelte:window` 上接 Esc。
   * 这三样都是 bits-ui 已经写对的东西，而且它还会做**键盘导航**（上下键、Home/End、
   * 输入首字母跳转、Esc 后把焦点还给触发点），手写版本一样都没有。
   *
   * ### 为什么是 `DropdownMenu` 而不是 `ContextMenu`
   *
   * 我们的模型是「事件坐标 → 单例菜单」，菜单的生命周期与**行节点**解耦
   * （虚拟列表会回收行）。`ContextMenu.Root` 的坐标只能由它自己的
   * `Trigger.oncontextmenu` 捕获（`menu.svelte.js` 里 `#handleOpen(e)` 读 clientX/Y，
   * 没有对外暴露 point 的入参），所以要把它挂在每一行上 —— 正是要避免的做法。
   *
   * `DropdownMenu` 的定位基准是**触发元素的 rect**，于是这里放一个 0×0 的锚点
   * 摆在点击坐标上：定位、碰撞翻转、外点关闭、Esc、焦点还原全部由 bits-ui 负责，
   * 我们只负责把坐标变成锚点位置。副作用是锚点是个一次性隐藏元素，
   * 菜单关闭时随组件一起卸载，行怎么回收都影响不到它。
   *
  * 坑（docs/24 §4.5）：`class` 不带 scoped 哈希 → 样式一律 `:global(.k-*)`；
  * `class:` 指令不能用在组件上 → 拼字符串。
  *
  * 坑 2（docs/25 §1 T3 实测）：**锚点必须 portal 到 body**。锚点是
  * `position: fixed`，一旦 Menu 被放进带 `transform` 的容器（bits-ui 的
  * Popover.Content 就用 transform 定位——EQ 抽屉是第一个这样的宿主），
  * fixed 退化为"相对该容器定位"，点击坐标变成容器内偏移，菜单整体
  * 飞出视口。portal 到 body 后 fixed 恒等于视口坐标；对原右键菜单
  * （宿主在 body 层）零行为差异。
  */

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
  }

  let { x, y, items, onclose }: Props = $props();

  /** 选中即执行。关闭由 bits-ui 的 `closeOnSelect` 负责，不再手动调 onclose */
  function pick(item: MenuItem): void {
    if (item.disabled) return;
    item.action();
  }
</script>

<!--
  `open` 恒为 true：调用方用 `{#if overlay.trackMenu}` 控制挂载，
  所以挂载即打开、卸载即关闭。`onOpenChange(false)` 只能是用户动作
  （Esc / 点外面 / 选中项）触发的，直接转成 onclose。
-->
<DropdownMenu.Root open={true} onOpenChange={(value) => (value ? undefined : onclose())}>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <!--
        0×0 锚点。`tabindex="-1"` 是必须的：默认的 button 会进 Tab 序列，
        用户按 Tab 会落到一个看不见的东西上。`pointer-events:none` 让它在点击
        坐标处不遮挡任何东西（菜单已经开着，下一次点击应该落到下层元素上）。
        portal 到 body：见坑 2（transform 祖先会让 fixed 失效）。
      -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -- 这是定位锚点，不是控件 -->
      <Portal>
        <span
          {...props}
          tabindex={-1}
          class="k-menu-anchor"
          style={`left:${x}px;top:${y}px`}
        ></span>
      </Portal>
    {/snippet}
  </DropdownMenu.Trigger>

    <DropdownMenu.Portal>
    <DropdownMenu.Content
      class={`k-menu${items.some((i) => i.selected) ? ' has-check' : ''}`}
      side="bottom"
      align="start"
      sideOffset={2}
      collisionPadding={8}
    >
      {#each items as item (item.label)}
        <DropdownMenu.Item
          class={`k-menu-item${item.danger ? ' danger' : ''}`}
          disabled={item.disabled ?? false}
          onSelect={() => pick(item)}
        >
          <!-- 选中标记（发布前 UI 审查 P10）：此前调用方往 label 里拼 '✓ '，
               读屏会把"勾"念出来。菜单里有任一选中项时整组预留勾位对齐。 -->
          <span class="k-menu-check" class:on={item.selected} aria-hidden="true">
            {#if item.selected}<Check size={13} />{/if}
          </span>
          {item.label}
        </DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  :global(.k-menu-anchor) {
    position: fixed;
    width: 0;
    height: 0;
    pointer-events: none;
  }

  :global(.k-menu) {
    z-index: var(--z-menu);
    /* 视觉沿用旧 .context-menu：190px 起、6px 内距、10px 圆角、毛玻璃 */
    min-width: 190px;
    padding: 6px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-4);
    box-shadow: var(--shadow-4);
    backdrop-filter: blur(var(--blur-pop));
  }

  :global(.k-menu-item) {
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border-radius: 7px;
    font-size: var(--fs-base);
    color: var(--text-1);
    cursor: default;
    user-select: none;
    outline: none;
  }

  /* 选中标记前位：菜单里有任一 selected 项时整组预留勾位对齐（.has-check
     由组件按 items 计算，见 Content 的 class） */
  :global(.k-menu-check) {
    display: none;
  }

  :global(.k-menu.has-check .k-menu-check) {
    display: inline-block;
    width: 14px;
    color: var(--accent-text);
    vertical-align: -2px;
  }

  /* 高亮使用 `data-highlighted`（键盘导航与 hover 都写这个属性），
     不能只写 :hover —— 否则用上下键移动时，视觉焦点不会跟着走 */
  :global(.k-menu-item[data-highlighted]) {
    background: var(--surface-hover);
  }

  :global(.k-menu-item[data-disabled]) {
    opacity: 0.35;
  }

  :global(.k-menu-item.danger) {
    color: var(--danger);
  }
</style>
