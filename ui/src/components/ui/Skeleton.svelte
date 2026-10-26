<script lang="ts">
  /**
   * 骨架屏：列表加载中的占位行 / 卡片块。
   *
   * 取代 `.skeleton` / `.shimmer` / `.tt-skeleton`。三种形态：
   * - `line`  一行文字条（宽度可给）
   * - `block` 一个矩形（封面、图表位）
   * - `row`   整行（左封面 + 两条文字），列表首屏用
   *
   * `count` 只对 `row` 有意义（一次渲染 N 行）。动画用 `prefers-reduced-motion` 关掉。
   */

  interface Props {
    kind?: 'line' | 'block' | 'row';
    /** 行/块的数量（`line` 默认 1，`row` 默认 6） */
    count?: number;
    width?: string;
    height?: string;
  }

  let { kind = 'line', count, width, height }: Props = $props();

  const n = $derived(count ?? (kind === 'row' ? 6 : 1));
</script>

{#if kind === 'row'}
  <div class="k-skeleton-rows" aria-hidden="true">
    {#each Array.from({ length: n }) as _, i (i)}
      <div class="k-skeleton-row">
        <div class="k-skel k-skel-cover"></div>
        <div class="k-skel-lines">
          <div class="k-skel" style="width: 42%"></div>
          <div class="k-skel" style="width: 26%; height: 8px"></div>
        </div>
      </div>
    {/each}
  </div>
{:else}
  <div class="k-skeleton-stack" aria-hidden="true">
    {#each Array.from({ length: n }) as _, i (i)}
      <div
        class="k-skel"
        class:block={kind === 'block'}
        style:width={width ?? (kind === 'line' ? '100%' : undefined)}
        style:height={height ?? (kind === 'line' ? '10px' : '64px')}
      ></div>
    {/each}
  </div>
{/if}

<style>
  .k-skeleton-stack,
  .k-skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .k-skel {
    border-radius: var(--radius-sm);
    background: var(--surface-hover);
    animation: k-shimmer 1.4s ease-in-out infinite;
  }

  .k-skel.block {
    border-radius: var(--radius-lg);
  }

  .k-skeleton-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    height: var(--row-h);
  }

  .k-skel-cover {
    width: 34px;
    height: 34px;
    border-radius: var(--radius-sm);
    flex: none;
  }

  .k-skel-lines {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
  }

  .k-skel-lines .k-skel {
    height: 10px;
  }

  @keyframes k-shimmer {
    0%,
    100% {
      opacity: 0.55;
    }
    50% {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .k-skel {
      animation: none;
      opacity: 0.7;
    }
  }
</style>
