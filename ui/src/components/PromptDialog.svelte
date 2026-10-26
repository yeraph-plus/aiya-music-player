<script lang="ts">
  import { untrack } from 'svelte';
  import Button from './ui/Button.svelte';
  import Dialog from './ui/Dialog.svelte';
  import Field from './ui/Field.svelte';

  /**
   * 输入框对话框（新建 / 重命名播放列表）。
   *
   * 与 `ConfirmDialog` 同构：`Dialog` 负责模态语义，这里只剩"一个字段 + 两个按钮"。
   * 原来那个 `<form onsubmit>` 也删了 —— 回车提交由 `Field.onenter` 提供，
   * 而提交按钮在 footer 里、根本不在 form 内，留着 form 只会让人以为它能
   * 接住回车（实际上接不住，回车走的是 form 内第一个 input 的隐式提交，
   * 那时按钮还是 `type=submit`，两处语义重叠）。
   *
   * `text` 用 `untrack` 初始化：`value` 只在打开那一刻有意义，
   * 否则父组件重渲染（例如歌单列表刷新）会把用户正在输的字覆盖掉。
   */

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

  const trimmed = $derived(text.trim());

  function submit(): void {
    if (!trimmed) return;
    onconfirm(trimmed);
  }
</script>

<Dialog
  {title}
  onclose={oncancel}
  autofocus={() => {
    input?.focus();
    // 重命名场景是「改已有名字」，全选后直接输入比让用户先删更快
    input?.select();
  }}
>
  <div class="prompt-body">
    <Field
      label={label || title}
      bind:ref={input}
      value={text}
      maxlength={80}
      oninput={(next) => (text = next)}
      onenter={submit}
    />
  </div>

  {#snippet actions()}
    <Button onclick={oncancel}>取消</Button>
    <Button variant={danger ? 'danger' : 'primary'} disabled={!trimmed} onclick={submit}>
      {confirmText}
    </Button>
  {/snippet}
</Dialog>

<style>
  .prompt-body {
    padding: 14px 16px 2px;
  }
</style>
