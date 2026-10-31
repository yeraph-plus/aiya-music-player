<script lang="ts">
  import type { Snippet } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { reducedMotion } from '../../lib/motion.svelte';

  interface Props {
    open: boolean;
    /** 关闭请求（Esc / 点遮罩） */
    onclose: () => void;
    /** 无障碍名称（role=dialog 的 aria-label） */
    label: string;
    children: Snippet;
  }

  let { open, onclose, label, children }: Props = $props();

  /**
   * 从窗口右缘滑出的悬浮抽屉（docs/25 §6）。
   *
   * 不使用 bits-ui Popover：气泡的定位由 floating-ui 在每次打开时测量计算，触发键
   * 位于窗口边缘时还需碰撞翻转，开合瞬间位置存在可见抖动。抽屉的几何是确定的
   * ——贴窗口右缘、固定宽度、固定上下外边距，不包含任何测量，因此不产生抖动。
   *
   * 几何：浮于主区之上，不与顶栏或播放条贴边——上下各让出一条栏高再加 10px
   * 外边距，四角圆角、整卡悬浮。背景使用 surface-4（不透明，见下方样式注释）。
   *
   * 焦点语义：打开时焦点移入面板（Esc 直接可用），关闭时还原到打开前的
   * 元素（触发键）——与 bits-ui Dialog 的行为对齐，只是手写。
   * 不做完整焦点圈禁（focus trap）：抽屉内容是"扫一眼队列/调一下 EQ"
   * 的旁路界面，不阻拦与主区的交互（点遮罩即关闭回主区）。
   */

  let panel = $state<HTMLElement | null>(null);
  let lastFocused: HTMLElement | null = null;

  $effect(() => {
    if (!open) return;
    lastFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // 等 DOM 挂上再聚焦（{#if} 刚翻开的那一帧 panel 还是 null）
    queueMicrotask(() => panel?.focus());
    return () => {
      lastFocused?.focus();
      lastFocused = null;
    };
  });

  function onkeydown(event: KeyboardEvent): void {
    if (open && event.key === 'Escape') {
      event.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={onkeydown} />

{#if open}
  <div class="sd-root">
    <!-- 遮罩：点了就是"我要回去"，比找关闭键近 -->
    <div
      class="sd-backdrop"
      transition:fade={{ duration: reducedMotion.value ? 0 : 150 }}
      onclick={onclose}
      aria-hidden="true"
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -- 面板容器，role 由脚本层管理焦点 -->
    <!-- 无 aria-modal：不拦截主区交互（遮罩只盖中段、播放条/顶栏仍可点），
         也不做焦点圈禁——标了 true 就是失实承诺，读屏会以为必须先离开抽屉 -->
    <div
      bind:this={panel}
      class="sd-panel"
      role="dialog"
      aria-label={label}
      tabindex="-1"
      transition:fly={{ x: 440, duration: reducedMotion.value ? 0 : 240 }}
    >
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .sd-root {
    position: fixed;
    inset: 0;
    z-index: var(--z-drawer); /* 浮层级：> .k-pop 55，< Menu 60（抽屉里的预设菜单要盖住面板） */
    /* 容器本身不拦截指针：拦截只归遮罩（且遮罩让出了顶栏与播放条）——
       抽屉开着时播放条触发键仍可点，否则互斥切换/再点关闭会被吃掉 */
    pointer-events: none;
  }

  .sd-backdrop {
    pointer-events: auto;
    position: absolute;
    /* 上下都让出栏高：顶栏有窗控与搜索、底栏有全部播放键，都不能被
       遮罩吃掉点击。Esc 与点主区遮罩是关抽屉的另外两条路。 */
    inset: var(--titlebar-h) 0 var(--player-h) 0;
    background: color-mix(in srgb, var(--shadow) 45%, transparent);
  }

  .sd-panel {
    pointer-events: auto;
    position: absolute;
    /* 悬浮卡：上下各让出栏高并保留 10px 边距，右侧等距——四边均不与窗口边缘
       或其他壳层贴边 */
    top: calc(var(--titlebar-h) + 10px);
    bottom: calc(var(--player-h) + 10px);
    right: 10px;
    width: min(380px, 92vw);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    /* 不透明：抽屉属内容密集的旁路界面，半透明时下层运动的频谱会使队列行难以
     * 阅读。使用 surface-4——token 表中面向"下方有列表运动的浮层"的最实一档
     * （97/98%，视觉上等同不透明），菜单同样使用该档。 */
    background: var(--surface-4);
    border: 1px solid var(--line);
    border-radius: 14px;
    box-shadow: var(--shadow-4-left);
    outline: none;
  }
</style>
