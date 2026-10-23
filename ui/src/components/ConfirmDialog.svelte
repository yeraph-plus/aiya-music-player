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

  // 焦点默认落在取消按钮：危险操作防手滑回车误确认，普通操作也是多按一次 Tab 的成本
  let cancelButton = $state<HTMLButtonElement | null>(null);
  $effect(() => {
    cancelButton?.focus();
  });
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape') oncancel();
    // 焦点在按钮上时 Enter 由按钮自身的 click 语义处理，这里再发一次会双触发
    if (event.key === 'Enter' && event.target instanceof HTMLButtonElement) return;
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
    <button class="btn" bind:this={cancelButton} onclick={oncancel}>取消</button>
    <button class="btn primary" class:danger onclick={onconfirm}>{confirmText}</button>
  </div>
</div>
