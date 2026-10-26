<script lang="ts">
  import Button from './ui/Button.svelte';
  import Dialog from './ui/Dialog.svelte';

  /**
   * 确认框（危险操作用它）。
   *
   * 现在只是 `Dialog` + 两个按钮的组合 —— 自己那段焦点陷阱（20 行手写
   * `FOCUSABLE_SELECTOR` / `trapFocus`）与 `svelte:window` 上的 Enter 处理都删了：
   * 前者 bits-ui 的 FocusScope 做得更全（还会还原焦点、锁滚动），后者是个**双触发源**
   * （焦点在按钮上时 Enter 既走原生 click 又走 window 处理器，靠 `instanceof HTMLButtonElement`
   * 才没炸）。
   *
   * **焦点默认落在「取消」**：危险操作的默认动作必须是"什么都不做"，
   * 回车误确认的代价是不可逆的。这条靠 `autofocus` 指定元素来实现，
   * 而不是靠拦截 Enter —— 拦截按键会和浏览器原生激活打架。
   */

  interface Props {
    title: string;
    message: string;
    confirmText?: string;
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { title, message, confirmText = '确定', danger = false, onconfirm, oncancel }: Props = $props();

  let cancelEl = $state<HTMLButtonElement | null>(null);
</script>

<Dialog {title} description={message} onclose={oncancel} autofocus={() => cancelEl?.focus()}>
  {#snippet actions()}
    <Button bind:ref={cancelEl} onclick={oncancel}>取消</Button>
    <Button variant={danger ? 'danger' : 'primary'} onclick={onconfirm}>{confirmText}</Button>
  {/snippet}
</Dialog>
