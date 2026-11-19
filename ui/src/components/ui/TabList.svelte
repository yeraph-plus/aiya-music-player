<script lang="ts">
  import { Tabs } from 'bits-ui';
  import type { ComponentProps } from 'svelte';
  import type TextTabs from './TextTabs.svelte';

  let { items, label, variant = 'underline' }: Pick<ComponentProps<typeof TextTabs>, 'items' | 'label' | 'variant'> = $props();
</script>

<Tabs.List class={`text-tabs ${variant}`} aria-label={label}>
  {#each items as tab (tab.id)}
    <Tabs.Trigger class="text-tab" value={tab.id} disabled={tab.disabled} title={tab.title}>
      {tab.label}
      {#if tab.count !== undefined}<span class="text-tab-count">{tab.count}</span>{/if}
    </Tabs.Trigger>
  {/each}
</Tabs.List>
