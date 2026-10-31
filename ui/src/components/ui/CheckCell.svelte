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
     * 勾选切换。鼠标点击与键盘 Space/Enter 均经由此回调。
     *
     * 键盘事件须拦截在 Checkbox 上：bits-ui 的 `Checkbox.Root` 对 Space/Enter 只
     * 翻转内部状态，不派发 click，受控模型因此收不到事件，表现为勾选后立即复位。
     * 该处理集中在本组件内，四个调用点（两个表格的表头与行内）共用同一实现。
     * `KeyboardEvent` 同样携带 shiftKey/ctrlKey，Shift 连选不受影响。
     */
    ontoggle: (event: MouseEvent | KeyboardEvent) => void;
  }

  let {
    checked,
    indeterminate = false,
    label,
    disabled = false,
    // 不设默认值：role="cell" 要求存在 row 或 grid 祖先，IndexPanel 在 ul/li 内
    // 使用时，默认值会产生非法 ARIA。表格调用方（TrackList / PlaylistList /
    // TrackTable）显式传 'cell' 或 'columnheader'。
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
