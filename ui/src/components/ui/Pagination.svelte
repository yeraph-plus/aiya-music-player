<script lang="ts">
  import { Pagination } from 'bits-ui';
  import { ChevronLeft, ChevronRight } from '@lucide/svelte';

  /**
   * 列表底部分页，用于专辑与艺人宫格页。
   *
   * 采用前端分页的原因：后端 facets 命令为冻结接口，单次最多返回 `FACET_LIMIT`
   * （500）条加 total，无 offset 参数。聚合本身为毫秒级，对已拉取的数据做展示层
   * 分页即可满足；若 500 条不足，应修改后端增加 offset，而非在此伪造分页。
   *
   * 页码序列由 bits-ui 的 Pagination 生成：首页与末页恒显示，当前页 ±1 恒显示，
   * 中段超出折叠为省略号（总页数 ≤7 时全量平铺）。同时提供页码间方向键漫游、
   * data-selected 与 aria-current 联动、首末页禁用。对外 API 为
   * page / pageCount / onchange。
   */

  interface Props {
    /** 当前页（1 起） */
    page: number;
    pageCount: number;
    onchange: (page: number) => void;
  }

  let { page, pageCount, onchange }: Props = $props();
</script>

<!-- Root 渲染的是普通 div，此处补上 nav 语义 -->
<Pagination.Root
  count={pageCount}
  perPage={1}
  {page}
  onPageChange={onchange}
  class="k-pag"
  role="navigation"
  aria-label="分页"
>
  {#snippet children({ pages })}
    <Pagination.PrevButton class="k-pag-btn" aria-label="上一页">
      <ChevronLeft size={14} />
    </Pagination.PrevButton>

    {#each pages as item (item.key)}
      {#if item.type === 'page'}
        <Pagination.Page
          class="k-pag-num"
          page={item}
          aria-current={item.value === page ? 'page' : undefined}
        >
          {item.value}
        </Pagination.Page>
      {:else}
        <span class="k-pag-gap" aria-hidden="true">…</span>
      {/if}
    {/each}

    <Pagination.NextButton class="k-pag-btn" aria-label="下一页">
      <ChevronRight size={14} />
    </Pagination.NextButton>
  {/snippet}
</Pagination.Root>

<style>
  /*
   * bits-ui 组件的 class 不带 scoped 哈希 → 样式一律 :global(.k-*)。
   * 与 Menu.svelte 同一条规矩。
   */
  :global(.k-pag) {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    /* 上方留呼吸（下方由页面的 padding-bottom 出，保持上下占位等高） */
    padding: 12px 0 0;
  }

  :global(.k-pag button) {
    display: grid;
    place-items: center;
    min-width: 26px;
    height: 26px;
    padding: 0 4px;
    border: none;
    border-radius: 6px;
    background: none;
    font-size: var(--fs-base);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  :global(.k-pag button:hover:not(:disabled)) {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  :global(.k-pag button:disabled) {
    opacity: var(--disabled-opacity);
    cursor: default;
  }

  :global(.k-pag-num[data-selected]) {
    background: var(--surface-hover);
    color: var(--text-1);
    font-weight: 600;
  }

  :global(.k-pag-gap) {
    min-width: 16px;
    text-align: center;
    font-size: var(--fs-base);
    color: var(--text-3);
  }
</style>
