<script lang="ts">
  /**
   * 统计块：一个数字 + 一行标签。
   *
   * 取代侧栏底部统计（`.sidebar-footer strong` + 四行文本）与诊断页的
   * 键值网格。数字一律 tabular-nums —— 统计数字之间不该因为 1 比 8 窄而抖动。
   *
   * `hint` 是标签下的补充（"3 张专辑"），不给就只显示两行。
   */

  interface Props {
    value: string | number;
    label: string;
    hint?: string;
    size?: 'sm' | 'md' | 'lg';
    /** 语义色：缺失/异常时用 danger */
    tone?: 'normal' | 'warn' | 'danger';
  }

  let { value, label, hint, size = 'md', tone = 'normal' }: Props = $props();
</script>

<div class="k-stat" class:sm={size === 'sm'} class:lg={size === 'lg'} class:warn={tone === 'warn'} class:danger={tone === 'danger'}>
  <span class="k-stat-value">{value}</span>
  <span class="k-stat-label">{label}</span>
  {#if hint}<span class="k-stat-hint">{hint}</span>{/if}
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
</style>
