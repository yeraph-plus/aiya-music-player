<script lang="ts">
  import { Search, X } from '@lucide/svelte';

  interface Props {
    search: string;
    onsearch: (value: string) => void;
    onclearsearch: () => void;
  }

  let { search, onsearch, onclearsearch }: Props = $props();
  let input = $state<HTMLInputElement | null>(null);

  export function focus(): void {
    input?.focus();
  }
</script>

<div class="topbar">
  <div class="search">
    <span class="search-icon" aria-hidden="true"><Search size={15} /></span>
    <input
      bind:this={input}
      type="search"
      placeholder="搜索本地曲库"
      aria-label="搜索本地曲库"
      value={search}
      oninput={(event) => onsearch(event.currentTarget.value)}
      spellcheck="false"
    />
    {#if search}
      <button class="search-clear" onclick={onclearsearch} aria-label="清除搜索" title="清除搜索">
        <X size={13} />
      </button>
    {/if}
  </div>
</div>
