import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app not found');

/**
 * 首帧主题：先定 `data-theme` 再挂载，避免浅色主题先画一帧深色。
 *
 * 现状（实测，别信旧注释）：主进程**没有**注入 `__AIYA_LOOK__`
 * （`grep __AIYA_LOOK__ src-tauri` 为空），所以这里恒为兜底值，
 * 浅色主题启动仍会闪一帧深色。真实值在 `ui.bootstrap()` 之后由
 * App 的 `$effect` 应用。
 *
 * 保留 `__AIYA_LOOK__` 的读取是为了主进程补注入时零改动——
 * 那件事记在 docs/18 §9 的遗留项里，不在本轮范围。
 */
const look = window.__AIYA_LOOK__ ?? { theme: 'dark', density: 'comfortable' };
document.documentElement.dataset.theme = look.theme;
document.documentElement.dataset.density = look.density;

const app = mount(App, { target });

export default app;
