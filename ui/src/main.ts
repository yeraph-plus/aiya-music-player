import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app not found');

// 先定主题再挂载：避免浅色主题启动时先画一帧深色。
// Tauri 用 initialization_script 注入 __AIYA_LOOK__（主进程已保存的主题/密度），
// 浏览器预览与降级路径回退到 compat api 的同步取值，最后兜底深色。
const look = window.__AIYA_LOOK__ ?? window.api.app.initialLook;
document.documentElement.dataset.theme = look.theme;
document.documentElement.dataset.density = look.density;

const app = mount(App, { target });

export default app;
