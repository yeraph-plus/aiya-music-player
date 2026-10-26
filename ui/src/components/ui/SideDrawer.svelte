<script lang="ts">
  import type { Snippet } from 'svelte';
  import { fade, fly } from 'svelte/transition';

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
   * 从窗口左缘滑出的悬浮抽屉（docs/25 §6）。
   *
   * 为什么不用 bits-ui Popover（B5a 的队列/EQ 抽屉原壳）：气泡的定位由
   * floating-ui 每次打开时测量计算，触发键在窗口边缘时还要做碰撞翻转，
   * 实测开合瞬间位置有可见抖动。抽屉的几何是**确定的**——贴窗口左缘、
   * 全高、固定宽度，没有任何测量参与，天然不抖。
   *
   * 焦点语义：打开时焦点移入面板（Esc 直接可用），关闭时还原到打开前的
   * 元素（触发键）——与 bits-ui Dialog 的行为对齐，只是手写。
   * 不做完整焦点圈禁（focus trap）：抽屉内容是"扫一眼队列/调一下 EQ"
   * 的旁路界面，不阻拦与主区的交互（点遮罩即关闭回主区）。
   */

  let panel = $state<HTMLElement | null>(null);
  let lastFocused: HTMLElement | null = null;

  const reducedMotion =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

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
      transition:fade={{ duration: reducedMotion ? 0 : 150 }}
      onclick={onclose}
      aria-hidden="true"
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -- 面板容器，role 由脚本层管理焦点 -->
    <div
      bind:this={panel}
      class="sd-panel"
      role="dialog"
      aria-modal="true"
      aria-label={label}
      tabindex="-1"
      transition:fly={{ x: -440, duration: reducedMotion ? 0 : 240 }}
    >
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .sd-root {
    position: fixed;
    inset: 0;
    z-index: 56; /* 浮层级：> .k-pop 55，< Menu 60（抽屉里的预设菜单要盖住面板） */
    /* 容器本身不拦截指针：拦截只归遮罩（且遮罩让出了播放条高度）——
       抽屉开着时播放条触发键仍可点，否则互斥切换/再点关闭会被吃掉 */
    pointer-events: none;
  }

  .sd-backdrop {
    pointer-events: auto;
  }

  .sd-backdrop {
    position: absolute;
    /* 底部让出播放条高度：抽屉开着时播放条触发键仍可点（互斥切换/再点
       关闭不被遮罩吃掉），Esc 与点主区遮罩是关抽屉的另外两条路 */
    inset: 0 0 var(--player-h) 0;
    background: color-mix(in srgb, var(--shadow) 45%, transparent);
  }

  .sd-panel {
    pointer-events: auto;
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: min(380px, 92vw);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface-2);
    border-right: 1px solid var(--line);
    border-radius: 0 14px 14px 0;
    box-shadow: 18px 0 44px var(--shadow);
    outline: none;
  }
</style>
