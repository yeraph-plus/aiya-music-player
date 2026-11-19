<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import Button from './Button.svelte';
  import Tooltip from './Tooltip.svelte';

  type Props = Omit<ComponentProps<typeof Button>, 'ariaLabel' | 'variant' | 'iconOnly' | 'title'> & {
    label: string;
    variant?: 'ghost' | 'solid' | 'close';
    expanded?: boolean;
  };
  let { label, variant = 'ghost', expanded, ref = $bindable(null), ...rest }: Props = $props();
</script>

<Tooltip {label}>
  {#snippet trigger(props)}
    <Button {...props} {...rest} bind:ref {variant} iconOnly ariaLabel={label} aria-expanded={expanded ?? rest['aria-expanded']} />
  {/snippet}
</Tooltip>
