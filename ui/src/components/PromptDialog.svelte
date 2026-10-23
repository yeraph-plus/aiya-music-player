<script lang="ts">
  import { untrack } from 'svelte';

  interface Props {
    title: string;
    label?: string;
    value?: string;
    confirmText?: string;
    danger?: boolean;
    onconfirm: (value: string) => void;
    oncancel: () => void;
  }

  let { title, label = '', value = '', confirmText = '确定', danger = false, onconfirm, oncancel }: Props =
    $props();

  let text = $state(untrack(() => value));
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
    input?.select();
  });

  function submit(event: Event): void {
    event.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    onconfirm(trimmed);
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={oncancel}></div>
<div class="modal prompt" role="dialog" aria-label={title}>
  <header>
    <strong>{title}</strong>
    <button class="icon-btn" onclick={oncancel} aria-label="关闭">×</button>
  </header>
  <form onsubmit={submit}>
    {#if label}<label class="field-label" for="prompt-input">{label}</label>{/if}
    <input
      id="prompt-input"
      class="text-input"
      bind:this={input}
      bind:value={text}
      maxlength="80"
      spellcheck="false"
      onkeydown={(event) => {
        if (event.key === 'Escape') oncancel();
        event.stopPropagation();
      }}
    />
    <div class="modal-actions">
      <button type="button" class="btn" onclick={oncancel}>取消</button>
      <button type="submit" class="btn primary" class:danger disabled={!text.trim()}>{confirmText}</button>
    </div>
  </form>
</div>
