<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Dialog } from 'bits-ui';
  import './dialog.css';
  import { X } from '@lucide/svelte';
  import { t } from '../../lib/i18n.svelte';
  import IconButton from './IconButton.svelte';

  /**
   * 对话框外壳：标题、可选说明、主体与动作区。
   *
   * 焦点管理交给 bits-ui：`Dialog.Content` 组合了 FocusScope、DismissibleLayer、
   * EscapeLayer 与 ScrollLock，提供 Tab 循环、关闭后还原焦点、滚动锁与
   * aria-modal 下的背景 inert。手写焦点陷阱通常只能覆盖 Tab 循环。
   *
   * ### 与调用方的约定
   *
   * - 挂载即打开（`open` 恒为 true）：调用方以 `{#if overlay.dialog?.kind}` 控制
   *   挂载，卸载即关闭。因此本组件不接收 `open` prop——额外的受控状态会产生
   *   "已关闭但仍挂载"的状态。
   * - `autofocus` 为回调而非选择器：元素引用由调用方持有（`bind:this`），传选择器
   *   还需在本组件内 `querySelector`。未提供时由 FocusScope 选取第一个可聚焦元素。
   * - Enter 不在此统一接管：焦点位于按钮时回车由浏览器原生激活，再于 window 上
   *   补一次会双触发。危险操作的初始焦点落在「取消」（由调用方通过 autofocus
   *   指定），回车误确认由焦点位置规避，不通过拦截按键实现。
   */

  interface Props {
    /**
     * 标题。未提供时不渲染整个头部（标题与关闭键），用于极简壳，如歌单选择器
     * 点击行即添加，标题与关闭键无信息量，关闭走 Esc 或点击遮罩。
     * 无障碍仍需 Dialog.Title，此时以视觉隐藏形式使用 `label` 挂载。
     */
    title?: string;
    /** 无头部时的无障碍名称（视觉隐藏的 Dialog.Title 文本） */
    label?: string;
    /** 说明文字。给了才渲染 `Dialog.Description`，并自动接上 aria-describedby */
    description?: string;
    width?: 'sm' | 'md';
    bodyPadding?: 'default' | 'none';
    /** 异步提交期间阻止关闭按钮、Esc 与遮罩关闭。 */
    dismissible?: boolean;
    /** 打开后要聚焦的元素；不给则聚焦内容区第一个可聚焦元素 */
    autofocus?: () => void;
    onclose: () => void;
    /** 主体。可以不给（确认框只有一句说明，说明走 description） */
    children?: Snippet;
    /** 底部动作区（右对齐）。不给就没有 footer */
    actions?: Snippet;
  }

  let { title, label, description, width = 'sm', bodyPadding = 'default', dismissible = true, autofocus, onclose, children, actions }: Props = $props();
</script>

<Dialog.Root open={true} onOpenChange={(value) => (value ? undefined : onclose())}>
  <Dialog.Portal>
    <!-- 遮罩与面板共享模态层级。 -->
    <Dialog.Overlay class="k-dlg-overlay" />
    <Dialog.Content
      class={`k-dlg${width === 'md' ? ' md' : ''}`}
      escapeKeydownBehavior={dismissible ? 'close' : 'ignore'}
      interactOutsideBehavior={dismissible ? 'close' : 'ignore'}
      onOpenAutoFocus={(event) => {
        if (!autofocus) return;
        // 拦住 FocusScope 的默认选择，交给调用方指定的元素
        event.preventDefault();
        autofocus();
      }}
    >
      {#if title}
        <header class="k-dlg-head">
          <Dialog.Title class="k-dlg-title">{title}</Dialog.Title>
          <IconButton label={t('common.close')} size="sm" disabled={!dismissible} onclick={onclose}>
            <X size={14} />
          </IconButton>
        </header>
      {:else}
        <!-- bits-ui 的 aria-labelledby 指向 Title 元素，无头部也要有一颗（视觉隐藏） -->
        <Dialog.Title class="k-dlg-sr">{label ?? ''}</Dialog.Title>
      {/if}

      {#if description}
        <Dialog.Description class="k-dlg-desc">{description}</Dialog.Description>
      {/if}

      {#if children}
        <div class="k-dlg-body" class:flush={bodyPadding === 'none'}>
          {@render children()}
        </div>
      {/if}

      {#if actions}
        <footer class="k-dlg-actions">
          {@render actions()}
        </footer>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
