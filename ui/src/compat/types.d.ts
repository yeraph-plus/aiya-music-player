import type { ListDensity, ThemeMode } from '@aiya/contracts';

declare global {
  interface Window {
    /**
     * 由 Tauri `initialization_script` 在页面脚本运行前注入；
     * 浏览器预览（vite dev）下不存在，回退到 compat api 的同步取值。
     */
    __AIYA_LOOK__?: { theme: ThemeMode; density: ListDensity };
  }
}

export {};
