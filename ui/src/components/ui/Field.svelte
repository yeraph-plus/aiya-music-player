<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';

  /** 原生输入属性透传；过滤和提交只上报业务值，不保存受控值的副本。 */
  interface Props extends Omit<HTMLInputAttributes, 'value' | 'oninput' | 'children' | 'size' | 'prefix'> {
    label: string;
    value?: string;
    hint?: string;
    error?: string | null;
    mono?: boolean;
    hideLabel?: boolean;
    variant?: 'default' | 'search';
    prefix?: Snippet;
    trailing?: Snippet;
    oninput?: (value: string) => void;
    onenter?: () => void;
    inputFilter?: (value: string) => string;
    ref?: HTMLInputElement | null;
  }
  const uid = $props.id();
  let {
    label, value = '', hint, error, mono = false, hideLabel = false, variant = 'default',
    prefix, trailing, oninput, onenter, onkeydown, inputFilter, type = 'text', id,
    disabled = false, class: className = '', ref = $bindable(null), ...rest
  }: Props = $props();
  const fieldId = $derived(id ?? `k-field-${uid}`);
  const descId = $derived(`${fieldId}-desc`);
  const describedBy = $derived([rest['aria-describedby'], error || hint ? descId : null].filter(Boolean).join(' ') || undefined);
</script>

<div class={`k-field ${className}`} class:with-error={!!error} class:disabled class:search={variant === 'search'}>
  <label for={fieldId} class="k-field-label" class:sr-only={hideLabel}>{label}</label>
  <div class="k-field-box">
    {#if prefix}<span class="k-field-prefix" aria-hidden="true">{@render prefix()}</span>{/if}
    <input
      {...rest} bind:this={ref} id={fieldId} {type} {disabled} {value}
      class="k-field-input" class:mono class:custom-clear={!!trailing}
      aria-invalid={error ? 'true' : rest['aria-invalid']}
      aria-describedby={describedBy}
      oninput={event => {
        const input = event.currentTarget;
        const next = inputFilter ? inputFilter(input.value) : input.value;
        if (next !== input.value) {
          const caret = input.selectionStart;
          const position = caret === null ? null : inputFilter!(input.value.slice(0, caret)).length;
          input.value = next;
          if (position !== null) input.setSelectionRange(position, position);
        }
        oninput?.(next);
      }}
      onkeydown={event => {
        onkeydown?.(event);
        if (onenter && event.key === 'Enter' && !event.isComposing && event.keyCode !== 229 && !event.defaultPrevented) {
          event.preventDefault();
          onenter();
        }
      }}
    />
    {#if trailing}<span class="k-field-trailing">{@render trailing()}</span>{/if}
  </div>
  {#if error}<p class="k-field-msg error" id={descId}>{error}</p>
  {:else if hint}<p class="k-field-msg" id={descId}>{hint}</p>{/if}
</div>

<style>
  .k-field {display:flex;flex-direction:column;gap:var(--space-2);min-width:0;}
  .k-field-label {font-size:var(--fs-base);color:var(--text-2);line-height:1.5;}
  .k-field-box {display:flex;align-items:center;gap:var(--space-2);height:var(--control-h);padding:0 var(--space-3);border:1px solid var(--line);border-radius:var(--radius-md);background:var(--surface-input);transition:border-color var(--dur-fast) var(--ease),background var(--dur-fast) var(--ease);}
  .k-field-box:focus-within {border-color:var(--accent);background:var(--surface-hover);outline:2px solid var(--accent);outline-offset:2px;}
  .with-error .k-field-box {border-color:var(--danger);}
  .disabled {opacity:var(--disabled-opacity);}
  .k-field-input {flex:1;min-width:0;width:100%;height:100%;padding:0;border:none;outline:none;background:none;color:var(--text-1);font-size:var(--fs-md);line-height:1.4;}
  .k-field-input.mono {font-family:var(--font-mono);font-variant-numeric:tabular-nums;}
  .k-field-input::placeholder {color:var(--text-3);}
  .k-field-input:disabled {cursor:default;}
  .k-field-input.custom-clear::-webkit-search-cancel-button {display:none;}
  .k-field-input[type='date']::-webkit-calendar-picker-indicator {cursor:pointer;}
  .k-field-prefix, .k-field-trailing {display:inline-flex;align-items:center;flex:none;color:var(--text-2);}
  .search .k-field-box {border-radius:var(--radius-pill);padding-inline:var(--space-4) var(--space-1);}
  .k-field-msg {margin:0;font-size:var(--fs-base);color:var(--text-2);line-height:1.5;}
  .k-field-msg.error {color:var(--danger);}
  .sr-only {position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap;}
</style>
