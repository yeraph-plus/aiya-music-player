<script lang="ts">
  import { Progress } from 'bits-ui';

  /**
   * 进度条：扫描/导入的确定进度。
   *
   * 内核 `bits-ui/Progress.Root`：它只做一件事 —— 把
   * `role="progressbar"` + `aria-valuenow/min/max` 算对（含 `value=null` 的
   * "不确定"态，此时不带 valuenow）。填充宽度由我们用百分比画。
   *
   * 取代 `.progress` / `.progress > i`（扫描条）与导入面板里那两条内联进度。
   */

  interface Props {
    value: number | null;
    max?: number;
    /** 无障碍名称，如「扫描进度」 */
    label: string;
    /** 细条（列表顶部扫描条）vs 粗条（面板内） */
    size?: 'sm' | 'md';
  }

  let { value, max = 100, label, size = 'sm' }: Props = $props();

  const pct = $derived(value === null ? 0 : Math.max(0, Math.min(100, (value / max) * 100)));
</script>

<Progress.Root
  {value}
  {max}
  aria-label={label}
  class={`k-progress${size === 'md' ? ' md' : ''}${value === null ? ' indeterminate' : ''}`}
>
  <div class="k-progress-fill" style:width={`${pct}%`}></div>
</Progress.Root>

<style>
  :global(.k-progress) {
    position: relative;
    flex: 1;
    min-width: 40px;
    height: 3px;
    overflow: hidden;
    border-radius: var(--radius-pill);
    background: var(--surface-hover);
  }

  :global(.k-progress.md) {
    height: 6px;
  }

  :global(.k-progress-fill) {
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width var(--dur-fast) linear;
  }

  /* 不确定态：一条 30% 宽的亮块来回走，不假装知道进度 */
  :global(.k-progress.indeterminate .k-progress-fill) {
    width: 30% !important;
    animation: k-progress-slide 1.2s ease-in-out infinite;
  }

  @keyframes k-progress-slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(340%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.k-progress.indeterminate .k-progress-fill) {
      animation: none;
      width: 100% !important;
      opacity: 0.5;
    }
  }
</style>
