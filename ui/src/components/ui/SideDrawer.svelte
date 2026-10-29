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
   * 从窗口右缘滑出的悬浮抽屉（docs/25 §6）。
   *
   * 为什么不用 bits-ui Popover（B5a 的队列/EQ 抽屉原壳）：气泡的定位由
   * floating-ui 每次打开时测量计算，触发键在窗口边缘时还要做碰撞翻转，
   * 实测开合瞬间位置有可见抖动。抽屉的几何是**确定的**——贴窗口右缘、
   * 固定宽度、固定上下外边距，没有任何测量参与，天然不抖。
   *
   * 几何：浮在主区之上、**不与顶栏/播放条贴边**——上下各让出一条栏高
   * 再加 10px 外边距，四角圆角、整卡悬浮（与窗控三键所在顶栏、播放条
   * 都保持呼吸距离）。背景用 surface-4（不透明，见下方样式注释）。
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
    <!-- 无 aria-modal：不拦截主区交互（遮罩只盖中段、播放条/顶栏仍可点），
         也不做焦点圈禁——标了 true 就是失实承诺，读屏会以为必须先离开抽屉 -->
    <div
      bind:this={panel}
      class="sd-panel"
      role="dialog"
      aria-label={label}
      tabindex="-1"
      transition:fly={{ x: 440, duration: reducedMotion ? 0 : 240 }}
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
    /* 悬浮卡：上下让出栏高再留 10px 呼吸边，右侧同宽——四边都不贴
       窗口/其他壳层边缘（用户反馈：不和顶栏、播放条贴在一起） */
    top: calc(var(--titlebar-h) + 10px);
    bottom: calc(var(--player-h) + 10px);
    right: 10px;
    width: min(380px, 92vw);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    /* **不透明**（用户拍板，撤掉与顶栏同档的 95% 半透明）：抽屉是内容
     * 密集的旁路界面，底下频谱在动时半透明会让队列行读不稳。用
     * surface-4——token 表里专门给"底下有列表在动的浮层"的最实一档
     * （97/98%，视觉等同不透明），菜单也是这一档。 */
    background: var(--surface-4);
    border: 1px solid var(--line);
    border-radius: 14px;
    box-shadow: -18px 0 44px var(--shadow);
    outline: none;
  }
</style>
