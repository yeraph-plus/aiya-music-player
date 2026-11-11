import type { AmbientScheme, ThemeMode } from './lib/settings';

/**
 * 全局环境声明。**没有 `window.api`**：UI 只走 `lib/ipc/client.ts`
 * 的 typed invoke，没有预加载的巨型 API 对象。
 */
declare global {
  interface Window {
    /**
     * 由宿主初始化脚本在页面脚本运行前注入（目前**实际未注入**，见 main.ts
     * 的注释与 遗留项）；浏览器预览下也不存在。
     * 保留声明是为了宿主补注入时零改动。
     */
    __AIYA_LOOK__?: { theme: ThemeMode; ambient?: AmbientScheme; uiScale?: number };
  }
}

export {};
