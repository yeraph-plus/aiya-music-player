<script lang="ts">
  import { onMount } from 'svelte';
  import { ChevronLeft, Copy, Minus, Search, Square, X } from '@lucide/svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Field from '../ui/Field.svelte';
  import { registerSearchFocus } from '../../lib/focus-search';
  import { t } from '../../lib/i18n.svelte';
  import {
    closeWindow,
    isNative,
    isWindowMaximized,
    minimizeWindow,
    onWindowResized,
    toggleMaximizeWindow,
  } from '../../lib/window-chrome';
  import { nav } from '../../stores/nav.svelte';
  import { ui } from '../../stores/ui.svelte';

  /**
   * 顶栏：唯一横贯全宽的全局栏。
   *
   * 一行四项：应用名、返回上一页、搜索框、窗口三键。
   * 搜索置于此处而非页面内：搜索属全局能力，页面级容器会随滚动
   * 移出视野、随路由卸载，而搜索需常驻。本层不设可扩展的页面级父栏，避免其成为
   * 归属不明的控件的默认去处。
   *
   * 返回键读取 `nav.canGoBack`（与侧栏共用同一历史栈），不自行推断上一页。
   */

  let maximized = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  onMount(() => {
    // 搜索框的焦点注册放在本组件内：触发按钮与输入框同属一处，不需要
    // 额外的 bind:this + $effect 把引用从壳传进来
    const disposeSearch = registerSearchFocus(() => input?.focus());
    if (!isNative) return disposeSearch;

    // 最大化状态可能被系统改（拖到屏幕边缘吸附、Win+↑），不能只信本地开关
    const syncMaximized = (): void => {
      void isWindowMaximized().then((value) => (maximized = value));
    };
    syncMaximized();
    const stopResizeWatch = onWindowResized(syncMaximized);

    return () => {
      disposeSearch();
      stopResizeWatch();
    };
  });
</script>

<!-- `data-tauri-drag-region="deep"` = 整个子树可拖（tauri 2.12.5 `drag.js` 的三种取值之一：`deep` 子树 / 裸属性仅自身 / `false` 禁用）。可点元素（`button`/`input`/ `select`/`textarea`/带
`tabindex≠-1` 的元素、交互 role）在**没有**该属性时会阻断冒泡路径上的拖拽 —— 所以按钮天然不拖，不需要给每个按钮做例外。双击标题栏由 tauri 内置走 `internal_toggle_maximize`。 -->
<header class="titlebar" data-tauri-drag-region="deep">
  <div class="tb-brand">
    <span class="tb-mark">A</span>
    <strong>AIYA</strong>
    <span class="tb-sub">MUSIC</span>
  </div>

  <div class="tb-back">
    <IconButton label={t('shell.titlebar.back')} disabled={!nav.canGoBack} onclick={() => nav.back()}>
      <ChevronLeft size={17} />
    </IconButton>
  </div>

  <div class="tb-search" data-tauri-drag-region="false">
    <Field label={t('shell.titlebar.search')} hideLabel variant="search" type="search"
      bind:ref={input} value={ui.search} placeholder={t('shell.titlebar.search')}
      title={t('shell.titlebar.searchTip')} spellcheck={false} oninput={value => nav.search(value)}>
      {#snippet prefix()}<Search size={14} />{/snippet}
      {#snippet trailing()}
        {#if ui.search}<IconButton label={t('shell.titlebar.searchClear')} size="sm" round onclick={() => {nav.clearSearch();input?.focus();}}><X size={14} /></IconButton>{/if}
      {/snippet}
    </Field>
  </div>

  <div class="tb-drag"></div>

  <!-- 窗口三键是壳的能力：浏览器里没有窗口语义，整组不渲染——
  disabled 占位同样暗示可点。拖拽区属性在 web 下是惰性属性，无需分支 -->
  {#if isNative}
    <div class="window-controls">
      <IconButton label={t('shell.titlebar.minimize')} onclick={minimizeWindow}><Minus size={16} /></IconButton>
      <IconButton label={maximized ? t('shell.titlebar.restore') : t('shell.titlebar.maximize')} onclick={toggleMaximizeWindow}>
        {#if maximized}<Copy size={14} />{:else}<Square size={14} />{/if}
      </IconButton>
      <IconButton label={t('common.close')} variant="close" onclick={closeWindow}><X size={16} /></IconButton>
    </div>
  {/if}
</header>

<style>
  .titlebar {
    grid-row: 1;
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    /* 左内距交给 .tb-brand 自己管——它的定宽与侧栏对齐（见下） */
    padding: 0;
    min-width: 0;
    /* 半透明 95%（接近不透明）：应用级背景从下层透出。底色取 surface-2，
     * 播放条取 surface-3，同档不同阶，叠放后产生轻微色差。
     * 不使用 backdrop-filter：顶栏属持久壳层，覆盖在动画频谱 canvas 之上时
     * 每帧都需对整条重新模糊。
     * 不设边线：与主区的分隔由底色阶差承担。 */
    background: color-mix(in srgb, var(--surface-2) 95%, transparent);
    user-select: none;
  }

  /* 品牌 */

  /*
   * 品牌区定宽 = 侧栏宽：右缘正好压在侧栏分隔线的延长线上
   * 左内距与侧栏容器及导航项内距之和一致，品牌标记与导航图标起点对齐。
   */
  .tb-brand {
    flex: none;
    width: var(--sidebar-w);
    padding-left: var(--space-5);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    letter-spacing: 0.14em;
  }

  .tb-mark {
    display: inline-grid;
    place-items: center;
    width: var(--space-6);
    height: var(--space-6);
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--surface-1);
    font-size: var(--fs-md);
    font-weight: 700;
    letter-spacing: 0;
  }

  .tb-brand strong {
    font-size: var(--fs-lg);
    font-weight: 700;
  }

  .tb-sub {
    font-size: var(--fs-xs);
    color: var(--text-2);
    letter-spacing: 0.22em;
  }

  /* 返回 */

  .tb-back {
    flex: none;
    margin-left: calc(var(--space-6) - var(--space-3));
  }

  .tb-search {flex:0 1 22.5rem;min-width:7.5rem;}

  /* 唯一的拖拽余量：顶栏空白处都在这里 */
  .tb-drag {
    flex: 1;
    min-width: var(--space-4);
    align-self: stretch;
  }

  .window-controls {flex:none;display:flex;align-items:center;gap:var(--space-1);padding-right:var(--space-4);}
</style>
