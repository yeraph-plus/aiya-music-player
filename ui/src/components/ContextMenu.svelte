<script lang="ts">
  import type { MenuItem } from '../lib/menu';

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
  }

  let { x, y, items, onclose }: Props = $props();

  // 靠近右/下边缘时翻到另一侧，避免菜单被窗口裁掉
  const style = $derived.by(() => {
    const width = 208;
    const height = items.length * 30 + 12;
    const left = Math.min(x, window.innerWidth - width - 8);
    const top = Math.min(y, window.innerHeight - height - 8);
    return `left:${Math.max(8, left)}px;top:${Math.max(8, top)}px;`;
  });

  function run(item: MenuItem): void {
    if (item.disabled) return;
    item.action();
    onclose();
  }
</script>

<svelte:window onkeydown={(event) => event.key === 'Escape' && onclose()} />

<div
  class="menu-backdrop"
  role="presentation"
  onclick={onclose}
  oncontextmenu={(event) => {
    event.preventDefault();
    onclose();
  }}
></div>

<div class="context-menu" style={style} role="menu">
  {#each items as item (item.label)}
    <button
      class="menu-item"
      class:danger={item.danger}
      role="menuitem"
      disabled={item.disabled}
      onclick={() => run(item)}
    >
      {item.label}
    </button>
  {/each}
</div>
