<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Tooltip as BitsTooltip } from 'bits-ui';

  /**
   * 悬停提示。
   *
   * 和原生 `title` 的分工（**两条都要遵守**）：
   * 1. **不承载唯一信息**：`title` 里的内容若换成 Tooltip 才看得到，那键盘用户
   *    和触屏用户就永远看不到 —— 所以触发元素本身必须有 `aria-label` 或可见文字。
   *    这条是 `title` 属性天生满足的（读屏器会念），Tooltip 反而需要刻意补。
   * 2. **两者不并存**：同一个元素既有 `title` 又有 Tooltip，会先弹原生气泡再弹我们的，
   *    看起来像 bug。接了 Tooltip 就把 `title` 删掉。
   *
   * `delayDuration` 统一由 `App.svelte` 顶层的 `Tooltip.Provider` 控制（400ms）——
   * **只能放一处**：每个 Tooltip 自带一份的话，相邻按钮之间不会共享"刚从别的按钮移过来
   * 就不用再等"的计时，快速横扫一排图标每次都要等满 400ms。
   *
   * trigger 用 snippet 的理由与 `Popover` 相同（props 要落到调用方的元素上）。
   */

  type TriggerProps = Record<string, any>;

  interface Props {
    label: string;
    side?: 'top' | 'bottom' | 'left' | 'right';
    align?: 'start' | 'center' | 'end';
    trigger: Snippet<[TriggerProps]>;
  }

  let { label, side = 'top', align = 'center', trigger }: Props = $props();
</script>

<BitsTooltip.Root>
  <BitsTooltip.Trigger>
    {#snippet child({ props })}
      {@render trigger(props)}
    {/snippet}
  </BitsTooltip.Trigger>

  <BitsTooltip.Portal>
    <BitsTooltip.Content class="k-tip" {side} {align} sideOffset={6} collisionPadding={8}>
      {label}
    </BitsTooltip.Content>
  </BitsTooltip.Portal>
</BitsTooltip.Root>

<style>
  :global(.k-tip) {
    z-index: var(--z-tooltip);
    padding: 5px 8px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--surface-4);
    color: var(--text-1);
    font-size: var(--fs-sm);
    line-height: 1.4;
    box-shadow: var(--shadow-2);
    pointer-events: none;
  }
</style>
