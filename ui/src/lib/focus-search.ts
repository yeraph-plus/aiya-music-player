/**
 * 搜索框聚焦：模块级注册表。
 *
 * 为什么需要它：快捷键 `focusSearch` 要拿到 TopBar 里的 `<input>`，而 TopBar
 * 实例是视图里的 `bind:this`。把实例一路传到键盘处理（App → keyboard）
 * 意味着键盘层必须知道视图层——反过来才对。
 *
 * 注册表只有一个槽位：TopBar 是单例，挂载时注册、卸载时清空。
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
