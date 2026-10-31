<script lang="ts">
  import { DropdownMenu, Portal } from 'bits-ui';
  import { Check } from '@lucide/svelte';
  import type { MenuItem } from '../../lib/menu';

  /**
   * 右键菜单（数据驱动）：输入 `items`，输出菜单。
   *
   * 定位、碰撞翻转、外部点击关闭、Esc 处理与焦点还原全部由 bits-ui 承担，
   * 本组件只把点击坐标转换成锚点位置。bits-ui 同时提供键盘导航（上下键、
   * Home/End、首字母跳转、Esc 后焦点归还触发点）。
   *
   * ### 选用 `DropdownMenu` 而非 `ContextMenu` 的依据
   *
   * 本组件的模型是「事件坐标 → 单例菜单」，菜单生命周期与行节点解耦（虚拟列表
   * 会回收行）。`ContextMenu.Root` 的坐标只能由其自身的 `Trigger.oncontextmenu`
   * 捕获（内部读取 clientX/Y，未对外暴露坐标入参），因此必须挂到每一行上，
   * 与该模型冲突。
   *
   * `DropdownMenu` 的定位基准是触发元素的 rect，因此此处放置一个 0×0 锚点在
   * 点击坐标上。副作用是锚点为一次性隐藏元素，菜单关闭时随组件卸载，不受行
   * 回收影响。
   *
   * 约束（docs/24 §4.5）：组件的 `class` 不带 scoped 哈希，样式一律使用
   * `:global(.k-*)`；`class:` 指令不能用于组件，需拼接字符串。
   *
   * 锚点必须 portal 到 body：锚点为 `position: fixed`，若 Menu 被置于带
   * `transform` 的容器内（bits-ui 的 Popover.Content 以 transform 定位，EQ 抽屉
   * 即此类宿主），fixed 会退化为相对该容器定位，点击坐标成为容器内偏移，菜单
   * 整体移出视口。portal 到 body 后 fixed 恒等于视口坐标（docs/25 §1）。
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
        0×0 锚点。`tabindex="-1"` 为必需：默认 button 会进入 Tab 序列，焦点将落在
        不可见元素上。`pointer-events:none` 使其在点击坐标处不遮挡下层，菜单打开时
        下一次点击应落到下层元素。portal 到 body 的原因见文件头。
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
          <!-- 选中标记：勾选图标由组件渲染，避免调用方在 label 中拼接 '✓ '
               而被读屏念出。菜单中存在任一选中项时，整组预留勾位以保持对齐。 -->
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
    opacity: var(--disabled-opacity);
  }

  :global(.k-menu-item.danger) {
    color: var(--danger);
  }
</style>
