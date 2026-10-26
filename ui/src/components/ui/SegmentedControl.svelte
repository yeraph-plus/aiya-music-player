<script lang="ts">
  import { ToggleGroup } from 'bits-ui';

  /**
   * 分段控件：2–5 个互斥短选项（密度、输出模式、缓冲）。
   *
   * 内核 `bits-ui/ToggleGroup`（`type="single"`）：它是 @sveltejs 生态里少数
   * 把"单选 + 方向键 + `aria-pressed`"三件事都做对的实现。外观完全自研。
   *
   * 两个必须处理的细节：
   * 1. `onValueChange` 会收到 `undefined`（再点一次当前项时取消选中），
   *    而分段控件**永远有值** → 这里吃掉 undefined，回落到当前值。
   * 2. 值类型是 `string`，调用方在自己的联合类型里窄化（`SegmentedControl<T>`）。
   */

  interface Props {
    value: string;
    options: ReadonlyArray<{ value: string; label: string; title?: string }>;
    /** 值类型是 string：调用方在自己的联合类型里窄化（`v as ListDensity`）。
     *  不用组件泛型 —— Svelte 的 `generics` 属性在这里推导不出来（数组字面量
     *  先被推成 string[]），最后还是要断言，不如把断言写在调用点。 */
    onchange: (value: string) => void;
    /** 无障碍名称（这一组选的是什么） */
    label: string;
    disabled?: boolean;
    /** 紧凑档：设置页行内用默认，工具栏里可以用 sm */
    size?: 'sm' | 'md';
  }

  let { value, options, onchange, label, disabled = false, size = 'md' }: Props = $props();
</script>

<ToggleGroup.Root
  type="single"
  value={value}
  aria-label={label}
  {disabled}
  onValueChange={(next) => {
    // 取消选中（undefined）时保持原值：分段控件没有"都不选"这个状态
    if (next !== undefined) onchange(next);
  }}
  class={`k-seg${size === 'sm' ? ' sm' : ''}`}
>
  {#each options as opt (opt.value)}
    <ToggleGroup.Item value={opt.value} class="k-seg-item" title={opt.title}>
      {opt.label}
    </ToggleGroup.Item>
  {/each}
</ToggleGroup.Root>

<style>
  :global(.k-seg) {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
  }

  :global(.k-seg-item) {
    height: calc(var(--control-h) - 6px);
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    color: var(--text-2);
    font-size: var(--fs-sm);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  :global(.k-seg.sm .k-seg-item) {
    height: 22px;
    padding: 0 7px;
    font-size: var(--fs-xs);
  }

  :global(.k-seg-item:hover:not([disabled])) {
    color: var(--text-1);
  }

  :global(.k-seg-item[data-state='on']) {
    background: var(--surface-3);
    color: var(--text-1);
    box-shadow: 0 1px 2px var(--shadow);
  }

  :global(.k-seg-item[disabled]) {
    opacity: 0.4;
    cursor: default;
  }
</style>
