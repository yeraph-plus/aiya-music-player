<script lang="ts">
  import type { Snippet } from 'svelte';
  import { AlertDialog } from 'bits-ui';
  import './dialog.css';

  let { title, description, autofocus, onclose, actions, dismissible = true }: {
    title: string;
    description: string;
    autofocus: () => void;
    onclose: () => void;
    actions: Snippet;
    dismissible?: boolean;
  } = $props();
</script>

<AlertDialog.Root open={true} onOpenChange={open => { if (!open) onclose(); }}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="k-dlg-overlay" />
    <AlertDialog.Content class="k-dlg" escapeKeydownBehavior={dismissible ? 'close' : 'ignore'} onOpenAutoFocus={event => { event.preventDefault(); autofocus(); }}>
      <header class="k-dlg-head"><AlertDialog.Title class="k-dlg-title">{title}</AlertDialog.Title></header>
      <AlertDialog.Description class="k-dlg-desc">{description}</AlertDialog.Description>
      <footer class="k-dlg-actions">{@render actions()}</footer>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>
