<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 字段：标签 + 单行输入 + 说明 / 错误。
   *
   * 取代 `.field-label` / `.text-input` / `.search-box` 三处各自拼接
   * （标题栏搜索框、新建歌单、重命名歌单、诊断页路径）。
   *
   * 受控：`value` 由调用方持有，`oninput` 只上报新值。**不在内部存副本** ——
   * 内部副本会和防抖保存打架（用户快速输入时最后一次写盘可能落到中间态）。
   *
   * `error` 与 `hint` 共用一个描述位（`aria-describedby` 只指向正在显示的那个），
   * 否则读屏会把两句一起念出来。
   */

  interface Props {
    label: string;
    value?: string;
    placeholder?: string;
    hint?: string;
    error?: string | null;
    disabled?: boolean;
    /** 等宽字体：路径、快捷键、数字 */
    mono?: boolean;
    type?: 'text' | 'search' | 'number' | 'date';
    oninput?: (value: string) => void;
    /** 回车提交（搜索框、重命名对话框） */
    onenter?: () => void;
    /** 输入框右侧的内联内容（清除按钮、单位） */
    trailing?: Snippet;
    id?: string;
    /** 视觉上隐藏标签（标题栏搜索框这种"位置已说明用途"的场景） */
    hideLabel?: boolean;
    /** 最大字符数（歌单名 80、搜索词 200 这类由业务决定，零件只负责转达） */
    maxlength?: number;
    /** 输入框元素，用于 `autofocus` / `select()`（对话框拿到后自己决定要不要选中全文） */
    ref?: HTMLInputElement | null;
  }

  const uid = $props.id();
  let {
    label,
    value = '',
    placeholder,
    hint,
    error,
    disabled = false,
    mono = false,
    type = 'text',
    oninput,
    onenter,
    trailing,
    id,
    hideLabel = false,
    maxlength,
    ref = $bindable(null),
  }: Props = $props();

  const fieldId = $derived(id ?? `k-field-${uid}`);
  const descId = $derived(`${fieldId}-desc`);
</script>

<div class="k-field" class:with-error={!!error}>
  <label for={fieldId} class="k-field-label" class:sr-only={hideLabel}>{label}</label>
  <div class="k-field-box">
    <input
      bind:this={ref}
      id={fieldId}
      {type}
      {placeholder}
      {maxlength}
      {disabled}
      value={value}
      class="k-field-input"
      class:mono
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error || hint ? descId : undefined}
      oninput={(event) => oninput?.(event.currentTarget.value)}
      onkeydown={(event) => {
        if (event.key === 'Enter') onenter?.();
      }}
    />
    {#if trailing}<span class="k-field-trailing">{@render trailing()}</span>{/if}
  </div>
  {#if error}
    <p class="k-field-msg error" id={descId}>{error}</p>
  {:else if hint}
    <p class="k-field-msg" id={descId}>{hint}</p>
  {/if}
</div>

<style>
  .k-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .k-field-label {
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .k-field-box {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--control-h);
    padding: 0 var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-input);
    transition:
      border-color var(--dur-fast) var(--ease),
      background var(--dur-fast) var(--ease);
  }

  .k-field-box:focus-within {
    border-color: var(--accent);
    background: var(--surface-hover);
  }

  .k-field.with-error .k-field-box {
    border-color: var(--danger);
  }

  .k-field-input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: none;
    outline: none;
    background: none;
    color: var(--text-1);
    font-size: var(--fs-base);
  }

  .k-field-input.mono {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }

  .k-field-input::placeholder {
    color: var(--text-3);
  }

  /* 原生日期控件的内框留给自己画（type=date）；日历图标跟随 color-scheme */
  .k-field-input[type='date']::-webkit-calendar-picker-indicator {
    cursor: pointer;
  }

  .k-field-input:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .k-field-trailing {
    flex: none;
    display: flex;
    align-items: center;
    color: var(--text-3);
  }

  .k-field-msg {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-2);
    line-height: 1.5;
  }

  .k-field-msg.error {
    color: var(--danger);
  }

  /* 视觉隐藏但仍可被读屏读到：标题栏搜索框的 label */
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
