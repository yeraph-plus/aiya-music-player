import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';

/**
 * 浏览器预览（`npm run dev:ui`）的装配点：没有 Tauri 运行时就把假后端
 * 装进 `lib/ipc/transport`，让所有 `ipc.*` 与事件订阅照常工作。
 *
 * 用动态 import 而不是顶层静态引入：mock 在 Tauri 构建里是独立 chunk，
 * 真宿主永不加载，也不会把 20KB 假数据塞进主 bundle。
 *
 * `?failCmd=library_list` 可注入一条命令失败，用来手验错误条渲染。
 */
if (!('__TAURI_INTERNALS__' in window)) {
  const { installMock } = await import('./lib/ipc/mock');
  const failCmd = new URLSearchParams(window.location.search).get('failCmd');
  installMock(failCmd ? { failCommands: [failCmd] } : {});
}

const target = document.getElementById('app');
if (!target) throw new Error('#app not found');

/**
 * 首帧主题：先定 `data-theme` 再挂载，避免浅色主题先画一帧深色。
 *
 * 现状（实测，别信旧注释）：主进程**没有**注入 `__AIYA_LOOK__`
 * （`grep __AIYA_LOOK__ src-tauri` 为空），所以 Tauri 下这里恒为兜底值，
 * 浅色主题启动仍会闪一帧深色。真实值在 `ui.bootstrap()` 之后由
 * App 的 `$effect` 应用。预览模式下 mock 会写 `__AIYA_LOOK__`，所以那一侧
 * 不会闪。
 *
 * 保留 `__AIYA_LOOK__` 的读取是为了主进程补注入时零改动——
 * 那件事记在 docs/18 §9 的遗留项里，不在本轮范围。
 */
const look = window.__AIYA_LOOK__ ?? { theme: 'dark', density: 'comfortable' };
document.documentElement.dataset.theme = look.theme;
document.documentElement.dataset.density = look.density;

const app = mount(App, { target });

export default app;
