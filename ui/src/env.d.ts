import type { AmbientScheme, ListDensity, ThemeMode } from './lib/settings';

/**
 * 全局环境声明。**没有 `window.api`**：批次 2 之后 UI 只走 `lib/ipc/client.ts`
 * 的 typed invoke，旧的 `AiyaApi` 巨型预加载对象随 `compat/` 一起删除。
 */
declare global {
  interface Window {
    /**
     * 由宿主初始化脚本在页面脚本运行前注入（目前**实际未注入**，见 main.ts
     * 的注释与 docs/18 §9 遗留项）；浏览器预览下也不存在。
     * 保留声明是为了宿主补注入时零改动。
     */
    __AIYA_LOOK__?: { theme: ThemeMode; density: ListDensity; ambient?: AmbientScheme };
  }
}

export {};
