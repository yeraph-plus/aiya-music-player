<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Copy, Minus, Square, X } from '@lucide/svelte';

  const native = '__TAURI_INTERNALS__' in window;
  // 浏览器预览（mock 传输）下没有 Tauri 元数据，getCurrentWindow 会直接抛错，必须惰性创建
  const appWindow = native ? getCurrentWindow() : null;

  let maximized = $state(false);
  let unlisten: (() => void) | null = null;

  onMount(() => {
    if (!appWindow) return;
    void appWindow.isMaximized().then((value) => (maximized = value));
    // 最大化状态可能被系统改（拖到屏幕边缘吸附、Win+↑），不能只信本地开关
    void appWindow.onResized(() => {
      void appWindow.isMaximized().then((value) => (maximized = value));
    }).then((stop) => (unlisten = stop));
  });

  onDestroy(() => unlisten?.());

  async function toggleMaximize(): Promise<void> {
    await appWindow?.toggleMaximize();
  }
</script>

<!--
  `data-tauri-drag-region="deep"` = 整个子树可拖（tauri 2.12 drag.js 的三种取值
  之一：`deep` 子树、裸属性仅自身、`false` 禁用）。按钮是"可点击元素"，
  没带该属性时会**阻断**冒泡路径上的拖拽，所以不需要给每个按钮做例外。
  双击标题栏由 tauri 内置走 `internal_toggle_maximize`。
-->
<header class="titlebar" data-tauri-drag-region="deep">
  <div class="titlebar-brand">
    <span class="brand-mark">A</span>
    <strong>AIYA</strong>
    <span>MUSIC</span>
  </div>
  <span class="titlebar-caption">本地音乐播放器</span>
  <div class="window-controls">
    <button
      aria-label="最小化"
      title="最小化"
      disabled={!native}
      onclick={() => void appWindow?.minimize()}
    >
      <Minus size={15} />
    </button>
    <button
      aria-label={maximized ? '还原' : '最大化'}
      title={maximized ? '还原' : '最大化'}
      disabled={!native}
      onclick={() => void toggleMaximize()}
    >
      {#if maximized}<Copy size={13} />{:else}<Square size={12} />{/if}
    </button>
    <button
      class="window-close"
      aria-label="关闭"
      title="关闭"
      disabled={!native}
      onclick={() => void appWindow?.close()}
    >
      <X size={15} />
    </button>
  </div>
</header>
