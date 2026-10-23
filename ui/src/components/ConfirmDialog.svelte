<script lang="ts">
  interface Props {
    title: string;
    message: string;
    confirmText?: string;
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { title, message, confirmText = '确定', danger = false, onconfirm, oncancel }: Props = $props();
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape') oncancel();
    if (event.key === 'Enter') onconfirm();
  }}
/>

<div class="modal-backdrop" role="presentation" onclick={oncancel}></div>
<div class="modal confirm" role="dialog" aria-label={title}>
  <header>
    <strong>{title}</strong>
    <button class="icon-btn" onclick={oncancel} aria-label="关闭">×</button>
  </header>
  <p class="confirm-message">{message}</p>
  <div class="modal-actions">
    <button class="btn" onclick={oncancel}>取消</button>
    <button class="btn primary" class:danger onclick={onconfirm}>{confirmText}</button>
  </div>
</div>
