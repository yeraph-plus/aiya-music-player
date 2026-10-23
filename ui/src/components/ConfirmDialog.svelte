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
  let dialogEl = $state<HTMLDivElement | null>(null);
  /** 打开前的 document.activeElement，关闭时还原 */
  let restoreFocusTo: HTMLElement | null = null;

  $effect(() => {
    restoreFocusTo = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cancelButton?.focus();
    return () => restoreFocusTo?.focus();
  });

  const FOCUSABLE_SELECTOR =
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  /** 焦点陷阱：Tab/Shift+Tab 循环限制在对话框内（捕获阶段，先于窗口级 Enter/Escape 处理） */
  function trapFocus(event: KeyboardEvent): void {
    if (event.key !== 'Tab' || !dialogEl) return;
    const focusable = Array.from(dialogEl.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
      (el) => el.getClientRects().length > 0,
    );
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }
    const first = focusable[0]!;
    const last = focusable[focusable.length - 1]!;
    const active = document.activeElement;
    const inside = active instanceof HTMLElement && dialogEl.contains(active);
    if (!event.shiftKey && (!inside || active === last)) {
      event.preventDefault();
      first.focus();
    } else if (event.shiftKey && (!inside || active === first)) {
      event.preventDefault();
      last.focus();
    }
  }
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
<div class="modal confirm" role="dialog" aria-modal="true" aria-label={title} bind:this={dialogEl} onkeydowncapture={trapFocus}>
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
