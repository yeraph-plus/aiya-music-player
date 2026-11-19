<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { Button as BitsButton } from 'bits-ui';

  interface Props extends Omit<HTMLButtonAttributes, 'children'> {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'link' | 'solid' | 'close';
    size?: 'sm' | 'md' | 'lg';
    iconOnly?: boolean;
    round?: boolean;
    active?: boolean;
    pressed?: boolean;
    loading?: boolean;
    dot?: boolean;
    ariaLabel?: string;
    icon?: Snippet;
    children?: Snippet;
    ref?: HTMLButtonElement | null;
  }

  let {
    variant = 'secondary', size = 'md', iconOnly = false, round = false,
    active = false, pressed, loading = false, dot = false, disabled = false,
    ariaLabel, icon, children, type = 'button', class: className = '',
    ref = $bindable(null), ...rest
  }: Props = $props();
</script>

<BitsButton.Root
  {...rest}
  bind:ref={() => ref, node => ref = node as HTMLButtonElement | null}
  {type}
  class={`k-btn ${variant} ${size}${iconOnly ? ' k-icon-btn icon-only' : ''}${round ? ' round' : ''}${active ? ' on' : ''}${loading ? ' loading' : ''} ${className}`}
  disabled={disabled || loading}
  aria-label={ariaLabel ?? rest['aria-label']}
  aria-pressed={pressed ?? rest['aria-pressed']}
  aria-busy={loading || rest['aria-busy'] || undefined}
>
  {#if loading}<span class="k-spin" aria-hidden="true"></span>{/if}
  {#if icon}<span class="k-btn-icon" aria-hidden="true">{@render icon()}</span>{/if}
  {#if children}<span class="k-btn-label">{@render children()}</span>{/if}
  {#if dot}<span class="k-btn-dot" aria-hidden="true"></span>{/if}
</BitsButton.Root>

<style>
  :global(.k-btn) {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: var(--control-h);
    padding: 0 var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
    color: var(--text-1);
    font-size: var(--fs-md);
    line-height: 1.4;
    white-space: nowrap;
    transition: background var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
  }
  :global(.k-btn:hover:not(:disabled)) {background:var(--surface-hover);border-color:var(--line-strong);}
  :global(.k-btn:active:not(:disabled)) {background:var(--surface-active);}
  :global(.k-btn.sm) {height:var(--control-h-sm);padding:0 var(--space-3);font-size:var(--fs-base);border-radius:var(--radius-sm);}
  :global(.k-btn.lg) {height:var(--control-h-lg);}
  :global(.k-btn.primary) {background:var(--accent-weak);border-color:var(--accent-weak);color:var(--accent-text);}
  :global(.k-btn.primary:hover:not(:disabled)) {background:color-mix(in srgb,var(--accent) 26%,transparent);border-color:color-mix(in srgb,var(--accent) 34%,transparent);}
  :global(.k-btn.ghost), :global(.k-btn.close) {background:transparent;border-color:transparent;color:var(--text-2);}
  :global(.k-btn.ghost:hover:not(:disabled)) {background:var(--surface-hover);border-color:transparent;color:var(--text-1);}
  :global(.k-btn.close:hover:not(:disabled)) {background:var(--close-hover);border-color:transparent;color:var(--close-fg);}
  :global(.k-btn.danger) {color:var(--danger);border-color:color-mix(in srgb,var(--danger) 32%,transparent);}
  :global(.k-btn.danger:hover:not(:disabled)) {background:color-mix(in srgb,var(--danger) 12%,transparent);border-color:color-mix(in srgb,var(--danger) 48%,transparent);}
  :global(.k-btn.solid), :global(.k-btn.solid:hover:not(:disabled)) {background:var(--play-bg);border-color:transparent;color:var(--play-fg);}
  :global(.k-btn.solid:hover:not(:disabled)) {filter:brightness(1.06);}
  :global(.k-btn.on), :global(.k-btn[aria-pressed='true']), :global(.k-btn[aria-expanded='true']) {background:var(--accent-weak);color:var(--accent-text);}
  :global(.k-btn.icon-only) {width:var(--control-h);padding:0;border-color:transparent;}
  :global(.k-btn.icon-only.sm) {width:var(--control-h-sm);}
  :global(.k-btn.icon-only.lg) {width:var(--control-h-lg);}
  :global(.k-btn.round) {border-radius:var(--radius-pill);}
  :global(.k-btn.link), :global(.k-btn.link.sm) {height:auto;padding:0;border:none;border-radius:0;background:none;color:var(--accent-text);font-size:inherit;line-height:inherit;white-space:normal;}
  :global(.k-btn.link:hover:not(:disabled)) {background:none;color:var(--accent);text-decoration:underline;text-underline-offset:2px;}
  :global(.k-btn:disabled) {opacity:var(--disabled-opacity);cursor:default;}
  :global(.k-btn.loading) {cursor:progress;}
  .k-btn-label, .k-btn-icon, .k-spin {display:inline-flex;align-items:center;justify-content:center;}
  :global(.k-btn.loading) > .k-btn-label, :global(.k-btn.loading) > .k-btn-icon {opacity:0;}
  .k-btn-dot {position:absolute;right:var(--space-1);bottom:var(--space-1);width:0.3125rem;height:0.3125rem;border-radius:var(--radius-pill);background:var(--accent);}
  .k-spin {position:absolute;left:calc(50% - 0.375rem);top:calc(50% - 0.375rem);width:0.75rem;height:0.75rem;border-radius:var(--radius-pill);background:conic-gradient(transparent 0 25%,currentColor 75% 100%);mask:radial-gradient(circle,transparent 0 0.1875rem,#000 0.21875rem);animation:k-spin 720ms linear infinite;}
  @keyframes k-spin {to {transform:rotate(360deg);}}
  @media (prefers-reduced-motion:reduce) {.k-spin {animation:none;}}
</style>
