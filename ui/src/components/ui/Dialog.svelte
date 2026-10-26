<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Dialog } from 'bits-ui';
  import { X } from '@lucide/svelte';
  import IconButton from './IconButton.svelte';

  /**
   * 对话框外壳：标题 + 可选的说明 + 主体 + 动作区。
   *
   * 取代三个组件里各写一遍的「backdrop + role=dialog + 手写焦点陷阱」：
   * `ConfirmDialog.svelte` 里那 20 行 `FOCUSABLE_SELECTOR` + `trapFocus` 是
   * 手写的焦点环，它做对了 Tab 循环但没做**关闭后还原焦点**、没做滚动锁、
   * 也没做 aria-modal 下的背景 inert。bits-ui 的 `Dialog.Content` 四样都有
   * （FocusScope / DismissibleLayer / EscapeLayer / ScrollLock）。
   *
   * ### 与调用方约定
   *
   * - **挂载即打开**（`open` 恒 true）：三个对话框现在都是 `{#if overlay.dialog?.kind}` 控制的，
   *   卸载即关闭。所以这里不接受 `open` prop —— 多一个受控状态就多一处"关了但还挂着"的坑。
   * - **`autofocus` 是回调不是选择器**：调用方手里才有元素引用（`bind:this`），
   *   传个选择器进来还要先 `querySelector`，反而更绕。不给就交给 FocusScope 选第一个可聚焦元素。
   * - **Enter 不在这里统一接管**。焦点在按钮上时回车由浏览器原生激活，再在 window 上
   *   补一次就会双触发。危险操作的焦点默认落在「取消」（调用方用 autofocus 指定），
   *   所以"回车误确认"这条风险由焦点位置挡，不靠拦截按键。
   */

  interface Props {
    title: string;
    /** 说明文字。给了才渲染 `Dialog.Description`，并自动接上 aria-describedby */
    description?: string;
    width?: 'sm' | 'md';
    /** 打开后要聚焦的元素；不给则聚焦内容区第一个可聚焦元素 */
    autofocus?: () => void;
    onclose: () => void;
    /** 主体。可以不给（确认框只有一句说明，说明走 description） */
    children?: Snippet;
    /** 底部动作区（右对齐）。不给就没有 footer */
    actions?: Snippet;
  }

  let { title, description, width = 'sm', autofocus, onclose, children, actions }: Props = $props();
</script>

<Dialog.Root open={true} onOpenChange={(value) => (value ? undefined : onclose())}>
  <Dialog.Portal>
    <!-- 覆盖层：视觉沿用旧 .modal-backdrop（重色 + 4px 模糊） -->
    <Dialog.Overlay class="k-dlg-overlay" />
    <Dialog.Content
      class={`k-dlg${width === 'md' ? ' md' : ''}`}
      onOpenAutoFocus={(event) => {
        if (!autofocus) return;
        // 拦住 FocusScope 的默认选择，交给调用方指定的元素
        event.preventDefault();
        autofocus();
      }}
    >
      <header class="k-dlg-head">
        <Dialog.Title class="k-dlg-title">{title}</Dialog.Title>
        <IconButton label="关闭" size="sm" onclick={onclose}>
          <X size={14} />
        </IconButton>
      </header>

      {#if description}
        <Dialog.Description class="k-dlg-desc">{description}</Dialog.Description>
      {/if}

      {#if children}
        <div class="k-dlg-body">
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

<style>
  :global(.k-dlg-overlay) {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--backdrop);
    backdrop-filter: blur(4px);
  }

  :global(.k-dlg) {
    position: fixed;
    top: 50%;
    left: 50%;
    /* bits-ui 的浮层用 transform 做动画/定位，居中也走 transform，
       所以这里必须写成 translate(-50%, -50%) 而不是 margin */
    transform: translate(-50%, -50%);
    z-index: 51;
    width: 380px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: 14px;
    background: var(--surface-3);
    box-shadow: 0 30px 80px var(--shadow);
    backdrop-filter: blur(28px);
    outline: none;
  }

  :global(.k-dlg.md) {
    width: 460px;
    max-height: 70vh;
  }

  .k-dlg-head {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 12px 12px 16px;
    border-bottom: 1px solid var(--line);
  }

  /* Title 是 bits-ui 渲染的元素，不在本组件作用域里 → 样式也要 :global */
  :global(.k-dlg-title) {
    flex: 1;
    min-width: 0;
    font-size: 13.5px;
    font-weight: 600;
    color: var(--text-1);
  }

  :global(.k-dlg-desc) {
    flex: none;
    margin: 0;
    padding: 14px 16px 0;
    font-size: 12px;
    line-height: 1.7;
    color: var(--text-2);
  }

  .k-dlg-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .k-dlg-actions {
    flex: none;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 16px;
  }
</style>
