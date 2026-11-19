<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Popover } from 'bits-ui';

  let { open, onclose, label, children }: {
    open: boolean; onclose: () => void; label: string; children: Snippet;
  } = $props();
</script>

<!-- 静态定位保留四区几何；Bits 管理焦点、Esc 和嵌套浮层的关闭顺序。 -->
<Popover.Root {open} onOpenChange={value => {if (!value) onclose();}}>
  <Popover.Portal>
    <Popover.Overlay class="sd-backdrop" />
    <Popover.ContentStatic
      class="sd-panel" role="dialog" aria-label={label}
      trapFocus={false} preventScroll={false}
      onInteractOutside={event => {
        const target = event.target;
        if (target instanceof Element && target.closest('.titlebar, .player, .k-pop')) event.preventDefault();
      }}
    >
      {@render children()}
    </Popover.ContentStatic>
  </Popover.Portal>
</Popover.Root>

<style>
  :global(.sd-backdrop) {position:fixed;inset:var(--titlebar-h) 0 var(--player-h);z-index:var(--z-drawer);background:color-mix(in srgb,var(--shadow) 45%,transparent);animation:sd-fade var(--dur-fast) var(--ease);}
  :global(.sd-panel) {
    position:fixed;top:calc(var(--titlebar-h) + var(--space-4));bottom:calc(var(--player-h) + var(--space-4));right:var(--space-4);
    z-index:var(--z-drawer);width:min(23.75rem,calc(100vw - 2 * var(--space-4)));
    display:flex;flex-direction:column;overflow:hidden;background:var(--surface-4);border:1px solid var(--line);
    border-radius:var(--radius-xl);box-shadow:var(--shadow-4-left);outline:none;
    animation:sd-in var(--dur-base) var(--ease);
  }
  @keyframes sd-fade {from {opacity:0;}}
  @keyframes sd-in {from {opacity:0;transform:translateX(var(--space-5));}}
  @media (prefers-reduced-motion:reduce) {:global(.sd-backdrop), :global(.sd-panel) {animation:none;}}
</style>
