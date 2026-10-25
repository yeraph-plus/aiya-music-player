/**
 * 搜索框聚焦：模块级注册表。
 *
 * 为什么需要它：快捷键 `focusSearch`（`/`）要拿到 TitleBar 里的 `<input>`，
 * 而实例在组件内部（`bind:this`）。把实例一路传到键盘处理（App → keyboard）
 * 意味着键盘层必须知道视图层——反过来才对。
 *
 * 注册表只有一个槽位：TitleBar 是单例，挂载时注册、卸载时清空。
 * 2026-10-26 壳体收口后注册点在 `components/TitleBar.svelte` 的 `onMount` 里
 * （之前是 AppShell 用 `bind:this` 转一手 —— 搜索框与注册逻辑进了同一个组件，
 * 那次转手就没有意义了）。
 */
let focusFn: (() => void) | null = null;

/** 挂载时调用，返回清理函数（给 `$effect` 的 return 用） */
export function registerSearchFocus(fn: () => void): () => void {
  focusFn = fn;
  return () => {
    if (focusFn === fn) focusFn = null;
  };
}

/** 没挂载时静默无操作：快捷键按下时视图可能还在启动中 */
export function focusSearch(): void {
  focusFn?.();
}
