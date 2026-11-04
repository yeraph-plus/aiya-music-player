<script lang="ts">
  import { onMount } from 'svelte';
  import { ChevronLeft, Copy, Minus, Search, Square, X } from '@lucide/svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Tooltip from '../ui/Tooltip.svelte';
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

  <!-- 返回键用 `IconButton`（kit），**不套 Tooltip**：`IconButton` 的 `label` 同时产出 `aria-label` 与原生 `title`，再叠一层 Tooltip 会先弹原生再弹自己的（见
  `ui/Tooltip.svelte` 的分工约定：两者不并存）。要 styled 提示的地方用裸 `<button>` + Tooltip（窗控三个就是这样）。 -->
  <div class="tb-back">
    <IconButton label={t('shell.titlebar.back')} disabled={!nav.canGoBack} onclick={() => nav.back()}>
      <ChevronLeft size={17} />
    </IconButton>
  </div>

  <!-- 搜索框不使用 `Field`：`data-tauri-drag-region="false"` 须挂在胶囊容器这层 div，而 `Field` 的内层盒子由组件自身渲染，kit 不向内部 DOM 透传任意属性（开放该能力等于放弃封装）。此外它属于窗口 chrome 而非表单字段：无标签、30px 高、999 圆角，复用表单零件的统一性不足以让 kit
  为此增加属性透传。`data-tauri-drag-region="false"` 为必需：胶囊容器是 div（非可点元素），顶栏为深度拖拽区，缺少该属性时点击输入框左右内边距会触发窗口拖动。反向陷阱：给这里的任何元素加 `tabindex="0"` 也会让整条拖拽区静默失效。 -->
  <div class="tb-search" data-tauri-drag-region="false">
    <span class="tb-search-icon"><Search size={14} /></span>
    <input
      bind:this={input}
      type="search"
      placeholder={t('shell.titlebar.search')}
      title={t('shell.titlebar.searchTip')}
      aria-label={t('shell.titlebar.search')}
      value={ui.search}
      oninput={(event) => nav.search(event.currentTarget.value)}
      spellcheck="false"
    />
    {#if ui.search}
      <button
        class="tb-search-clear"
        aria-label={t('shell.titlebar.searchClear')}
        title={t('shell.titlebar.searchClear')}
        onclick={() => nav.clearSearch()}
      >
        <X size={13} />
      </button>
    {/if}
  </div>

  <div class="tb-drag"></div>

  <div class="window-controls">
    <Tooltip label={t('shell.titlebar.minimize')}>
      {#snippet trigger(props)}
        <button {...props} aria-label={t('shell.titlebar.minimize')} disabled={!isNative} onclick={minimizeWindow}>
          <Minus size={15} />
        </button>
      {/snippet}
    </Tooltip>
    <Tooltip label={maximized ? t('shell.titlebar.restore') : t('shell.titlebar.maximize')}>
      {#snippet trigger(props)}
        <button
          {...props}
          aria-label={maximized ? t('shell.titlebar.restore') : t('shell.titlebar.maximize')}
          disabled={!isNative}
          onclick={toggleMaximizeWindow}
        >
          {#if maximized}<Copy size={13} />{:else}<Square size={12} />{/if}
        </button>
      {/snippet}
    </Tooltip>
    <Tooltip label={t('common.close')}>
      {#snippet trigger(props)}
        <button
          {...props}
          class="window-close"
          aria-label={t('common.close')}
          disabled={!isNative}
          onclick={closeWindow}
        >
          <X size={15} />
        </button>
      {/snippet}
    </Tooltip>
  </div>
</header>

<style>
  .titlebar {
    grid-row: 1;
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 8px;
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
   * 左内距 = 侧栏容器 10px + 导航项内距 9px，品牌标记与导航文字同一条竖线起排。
   */
  .tb-brand {
    flex: none;
    width: var(--sidebar-w);
    padding-left: 19px;
    display: flex;
    align-items: center;
    gap: 6px;
    letter-spacing: 0.14em;
  }

  .tb-mark {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
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

  /*
   * 左缘 = 侧栏宽 + 22px：与 .page-scroll / .page-pad 的内容左边距同一条竖线。
   * **注意补偿顶栏 flex gap（8px）**：margin 是叠加在 gap 之上的，写 22px 实际
   * 落在 246px（实测），比页面内容左缘 238px 右偏 8px —— 之前"没对齐"就是它。
   * 按钮用 md 档（30px），与右侧搜索胶囊同高——同排两个控件高度一致才谈得上对齐。
   */
  .tb-back {
    flex: none;
    margin-left: 14px;
  }

  /* 搜索 */

  .tb-search {
    flex: 0 1 360px;
    min-width: 120px;
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface-input);
  }

  .tb-search:focus-within {
    /* 与 Field / Select 使用同一焦点语言（accent）；仅取 line-strong 会使同屏
       输入框出现两种焦点色 */
    border-color: var(--accent);
  }

  .tb-search-icon {
    flex: none;
    display: inline-flex;
    color: var(--text-2);
  }

  .tb-search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--text-1);
    font-size: var(--fs-base);
  }

  .tb-search input::placeholder {
    color: var(--text-2);
  }

  /* 去掉 type=search 的原生 ✕：右边已经有我们自己的清除键，两个会并排 */
  .tb-search input::-webkit-search-cancel-button {
    display: none;
  }

  .tb-search-clear {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: var(--text-2);
  }

  .tb-search-clear:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  /* 唯一的拖拽余量：顶栏空白处都在这里 */
  .tb-drag {
    flex: 1;
    min-width: 12px;
    align-self: stretch;
  }

  /* 窗口控制 */

  /* 三键做成**普通按钮**而不是通高热区：与返回键同用 IconButton 的 30px 方块节奏（radius-md + hover 底色），不再 `align-self:
     stretch` 撑满整条 54px 顶栏——通高块在浅色主题下 hover 时像把整条栏切成三段，视觉上过重。 */
  .window-controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: 10px;
  }

  .window-controls button {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-md);
    color: var(--text-2);
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  .window-controls button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .window-controls .window-close:hover:not(:disabled) {
    background: var(--close-hover);
    color: var(--close-fg);
  }

  .window-controls button:disabled {
    opacity: 0.35;
  }
</style>
