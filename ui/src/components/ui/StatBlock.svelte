<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 统计块：一个数字 + 一行标签。
   *
   * 取代侧栏底部统计（`.sidebar-footer strong` + 四行文本）、诊断页的
   * 键值网格，以及设置页手写的 `.stat-card`（发布前 UI 审查 P1：死件与
   * 手写替身并存）。数字一律 tabular-nums —— 统计数字之间不该因为 1 比 8
   * 窄而抖动。
   *
   * `hint` 是标签下的补充（"3 张专辑"），不给就只显示两行。
   *
   * `layout="row"` 是设置页的卡片形态：可选 `icon` 槽在左（accent 圆角方块），
   * 数字 + 标签在右，整块带卡片底。`warn` 时图标底随语义走 warn 色。
   */

  interface Props {
    value: string | number;
    label: string;
    hint?: string;
    size?: 'sm' | 'md' | 'lg';
    /** 语义色：缺失/异常时用 danger */
    tone?: 'normal' | 'warn' | 'danger';
    /** 卡片形态（设置页统计卡）：图标在左、文本在右、自带卡片底 */
    layout?: 'stack' | 'row';
    icon?: Snippet;
  }

  let { value, label, hint, size = 'md', tone = 'normal', layout = 'stack', icon }: Props = $props();
</script>

<div
  class="k-stat"
  class:sm={size === 'sm'}
  class:lg={size === 'lg'}
  class:warn={tone === 'warn'}
  class:danger={tone === 'danger'}
  class:row={layout === 'row'}
>
  {#if icon}<span class="k-stat-icon" aria-hidden="true">{@render icon()}</span>{/if}
  <span class="k-stat-text">
    <span class="k-stat-value">{value}</span>
    <span class="k-stat-label">{label}</span>
    {#if hint}<span class="k-stat-hint">{hint}</span>{/if}
  </span>
</div>

<style>
  .k-stat {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .k-stat-value {
    font-size: var(--fs-xl);
    font-weight: 600;
    line-height: 1.1;
    color: var(--text-1);
    font-variant-numeric: tabular-nums;
  }

  .k-stat.sm .k-stat-value {
    font-size: var(--fs-md);
    font-weight: 500;
  }

  .k-stat.lg .k-stat-value {
    font-size: var(--fs-hero);
  }

  .k-stat-label {
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .k-stat-hint {
    font-size: var(--fs-xs);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .k-stat.warn .k-stat-value {
    color: var(--warn);
  }

  .k-stat.danger .k-stat-value {
    color: var(--danger);
  }

  /* ---- row（设置页统计卡）---- */
  .k-stat.row {
    flex-direction: row;
    align-items: center;
    gap: 12px;
    padding: 12px 18px 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    min-width: 118px;
  }

  .k-stat-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent-text);
  }

  .k-stat.warn .k-stat-icon {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }

  .k-stat.row .k-stat-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .k-stat.row .k-stat-value {
    font-weight: 400; /* 用户拍板：数字不加粗，靠字号本身立住 */
    line-height: 1.15;
    white-space: nowrap;
  }
</style>
