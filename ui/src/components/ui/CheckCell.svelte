<script lang="ts">
  import { Check, Minus } from '@lucide/svelte';
  import { Checkbox } from 'bits-ui';

  interface Props {
    checked: boolean;
    /** 表头三态全选的半选态（行内勾选框永远 false） */
    indeterminate?: boolean;
    /** 无障碍名称（这一格勾的是什么 / 全选） */
    label: string;
    disabled?: boolean;
    /** 行内 cell / 表头 columnheader */
    role?: 'cell' | 'columnheader';
    /**
     * 勾选切换。鼠标点击与键盘 Space/Enter **都走这里**。
     *
     * 键盘必须拦在 Checkbox 上（B4-1 实测）：bits-ui 的 `Checkbox.Root`
     * 收 Space/Enter 只翻**内部** state、不派发 click，受控模型收不到 →
     * "勾上了又自己掉"。这个防护曾经复制粘贴在四个调用点（两个表格的
     * 表头+行内），B4-2c 收进本组件 —— 改语义只改这一处。
     * `KeyboardEvent` 同样有 `shiftKey`/`ctrlKey`，Shift 连选不丢。
     */
    ontoggle: (event: MouseEvent | KeyboardEvent) => void;
  }

  let {
    checked,
    indeterminate = false,
    label,
    disabled = false,
    // 不给默认值（发布前 UI 审查 P6.1）：role="cell" 要求 row/grid 祖先，
    // IndexPanel 在 ul/li 里用它时默认值会产出非法 ARIA。表格调用方
    // （TrackList/PlaylistList/TrackTable）显式传 'cell' / 'columnheader'。
    role,
    ontoggle,
  }: Props = $props();
</script>

<!-- 点击处理放在包装层而不是 Checkbox 上：bits-ui 自己的 click 处理器会同时跑，
     内部 checked 与我们的受控 prop 各翻一次，结果正好反相（实测：勾了的行
     显示未勾）。行级"点勾选框不是点行"的判断认 `.k-check`（见 TrackList/TrackTable）。
     tabindex=-1：可编程聚焦但不进 Tab 序列，真正的键盘入口是内层 button。 -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<span {role} class="k-check" tabindex="-1" onclick={(event) => ontoggle(event)}>
  <Checkbox.Root
    class="k-box"
    {checked}
    indeterminate={indeterminate}
    aria-label={label}
    {disabled}
    onkeydown={(event) => {
      // 见 Props.ontoggle 注释：Space/Enter 拦下来走统一语义
      if (event.key === ' ' || event.key === 'Enter') {
        event.preventDefault();
        event.stopPropagation();
        ontoggle(event);
      }
    }}
  >
    {#snippet children({ checked, indeterminate })}
      {#if indeterminate}<Minus size={11} />{:else if checked}<Check size={11} />{/if}
    {/snippet}
  </Checkbox.Root>
</span>
