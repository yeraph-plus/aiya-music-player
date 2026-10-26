<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { ChevronLeft, Copy, Minus, Search, Square, X } from '@lucide/svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Tooltip from '../ui/Tooltip.svelte';
  import { registerSearchFocus } from '../../lib/focus-search';
  import { nav } from '../../stores/nav.svelte';
  import { ui } from '../../stores/ui.svelte';

  /**
   * 宽 TitleBar：**唯一一条横贯全宽的全局栏**。
   *
   * 四件东西一行：应用名 / 返回上一页 / 搜索框 / 窗口三键。
   * 为什么搜索放在这里而不是页面里（docs/24 §3.1 修订 v2）：搜索是**全局**能力，
   * 页面级容器会随滚动走、随路由消失，而搜索必须常驻。之前那一级叫 TopBar，
   * 它一存在就必然持续吸收"不知道放哪"的东西 —— 所以这一级被删掉了。
   *
   * 返回键读 `nav.canGoBack`（与侧栏共用同一个历史栈），不是自己猜"上一页是谁"。
   */

  const native = '__TAURI_INTERNALS__' in window;
  // 浏览器预览（mock 传输）下没有 Tauri 元数据，getCurrentWindow 会直接抛错，必须惰性创建
  const appWindow = native ? getCurrentWindow() : null;

  let maximized = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  onMount(() => {
    // 搜索框的焦点注册从 AppShell 上移到这里：按钮和输入框在同一个组件里，
    // 不再需要一个 bind:this + $effect 把引用从壳传出来
    const disposeSearch = registerSearchFocus(() => input?.focus());
    if (!appWindow) return disposeSearch;

    void appWindow.isMaximized().then((value) => (maximized = value));
    // 最大化状态可能被系统改（拖到屏幕边缘吸附、Win+↑），不能只信本地开关
    const unlisten = appWindow.onResized(() => {
      void appWindow.isMaximized().then((value) => (maximized = value));
    });

    return () => {
      disposeSearch();
      void unlisten.then((stop) => stop());
    };
  });
</script>

<!--
  `data-tauri-drag-region="deep"` = 整个子树可拖（tauri 2.12.5 `drag.js` 的三种取值
  之一：`deep` 子树 / 裸属性仅自身 / `false` 禁用）。可点元素（`button`/`input`/
  `select`/`textarea`/带 `tabindex≠-1` 的元素、交互 role）在**没有**该属性时会阻断
  冒泡路径上的拖拽 —— 所以按钮天然不拖，不需要给每个按钮做例外。
  双击标题栏由 tauri 内置走 `internal_toggle_maximize`。
-->
<header class="titlebar" data-tauri-drag-region="deep">
  <div class="tb-brand">
    <span class="tb-mark">A</span>
    <strong>AIYA</strong>
    <span class="tb-sub">MUSIC</span>
  </div>

  <!--
    返回键用 `IconButton`（kit），**不套 Tooltip**：`IconButton` 的 `label` 同时产出
    `aria-label` 与原生 `title`，再叠一层 Tooltip 会先弹原生再弹自己的（见 `ui/Tooltip.svelte`
    的分工约定：两者不并存）。要 styled 提示的地方用裸 `<button>` + Tooltip（窗控三个就是这样）。
  -->
  <div class="tb-back">
    <IconButton label="返回上一页" size="sm" disabled={!nav.canGoBack} onclick={() => nav.back()}>
      <ChevronLeft size={16} />
    </IconButton>
  </div>

  <!--
    **搜索框不换 `Field`**（B3b-2 决定）：`data-tauri-drag-region="false"` 必须挂在
    胶囊这层 div 上，而 `Field` 的内层盒子由组件自己渲染、kit 不透传任意 DOM 属性
    （开口子等于放弃封装）；另外它是 chrome 不是表单字段 —— 无标签、30px 高、999 圆角。
    换来的统一性不值得让 kit 长出"透传属性"这条后门。

    `data-tauri-drag-region="false"` 是**必须的**：胶囊容器是个 div（不是可点元素），
    顶栏又是 deep 拖拽区 —— 不加这一行，点输入框左右的内边距会开始拖窗口。
    反向陷阱：给这里的任何元素加 `tabindex="0"` 也会让整条拖拽区静默失效。
  -->
  <div class="tb-search" data-tauri-drag-region="false">
    <span class="tb-search-icon"><Search size={14} /></span>
    <input
      bind:this={input}
      type="search"
      placeholder="搜索本地曲库"
      title="搜索本地曲库（按 / 聚焦）"
      aria-label="搜索本地曲库"
      value={ui.search}
      oninput={(event) => nav.search(event.currentTarget.value)}
      spellcheck="false"
    />
    {#if ui.search}
      <button
        class="tb-search-clear"
        aria-label="清除搜索"
        title="清除搜索"
        onclick={() => nav.clearSearch()}
      >
        <X size={13} />
      </button>
    {/if}
  </div>

  <div class="tb-drag"></div>

  <div class="window-controls">
    <Tooltip label="最小化">
      {#snippet trigger(props)}
        <button {...props} aria-label="最小化" disabled={!native} onclick={() => void appWindow?.minimize()}>
          <Minus size={15} />
        </button>
      {/snippet}
    </Tooltip>
    <Tooltip label={maximized ? '还原' : '最大化'}>
      {#snippet trigger(props)}
        <button
          {...props}
          aria-label={maximized ? '还原' : '最大化'}
          disabled={!native}
          onclick={() => void appWindow?.toggleMaximize()}
        >
          {#if maximized}<Copy size={13} />{:else}<Square size={12} />{/if}
        </button>
      {/snippet}
    </Tooltip>
    <Tooltip label="关闭">
      {#snippet trigger(props)}
        <button
          {...props}
          class="window-close"
          aria-label="关闭"
          disabled={!native}
          onclick={() => void appWindow?.close()}
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
    border-bottom: 1px solid var(--line);
    /* 半透明（docs/25 T4g 定档 95%——用户反馈要接近不透明）：全应用
     * 背景从底下透出来。底色用 surface-2（播放条是 surface-3）——同档
     * 不同阶，叠起来有一点色差变化。不用 backdrop-filter——它是持久壳层，
     * 垫着动画频谱 canvas 就是每帧全条重模糊（docs/25 §5 方案 B 红线）。 */
    background: color-mix(in srgb, var(--surface-2) 95%, transparent);
    user-select: none;
  }

  /* ---------------- 品牌 ---------------- */

  /*
   * 品牌区定宽 = 侧栏宽（docs/25 §2）：右缘正好压在侧栏分隔线的延长线上，
   * 左内距 = 侧栏容器 10px + 导航项内距 9px，品牌标记与导航文字同一条竖线起排。
   */
  .tb-brand {
    flex: none;
    width: var(--sidebar-w);
    padding-left: 19px;
    display: flex;
    align-items: center;
    gap: 5px;
    letter-spacing: 0.14em;
  }

  .tb-mark {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 5px;
    background: var(--accent);
    color: var(--surface-1);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0;
  }

  .tb-brand strong {
    font-size: 12px;
    font-weight: 700;
  }

  .tb-sub {
    font-size: 9px;
    color: var(--text-2);
    letter-spacing: 0.22em;
  }

  /* ---------------- 返回 ---------------- */

  /*
   * 左缘 = 侧栏宽 + 主区内容边距（--space-6，随密度 16/22/26px）：
   * 与 .page-pad 的内容左边距同一条竖线（docs/25 §2）。搜索框跟在后面同节奏。
   */
  .tb-back {
    flex: none;
    margin-left: var(--space-6);
  }

  /* ---------------- 搜索 ---------------- */

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
    border-color: var(--line-strong);
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
    font-size: 12.5px;
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

  /* ---------------- 窗口控制 ---------------- */

  .window-controls {
    flex: none;
    display: flex;
    align-items: stretch;
    align-self: stretch;
  }

  .window-controls button {
    width: 44px;
    display: grid;
    place-items: center;
    color: var(--text-2);
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
