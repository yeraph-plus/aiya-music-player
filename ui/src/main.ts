// 第一行，必须在任何会构造 zod schema 的模块之前（原因见该文件注释）
import './lib/zod-setup';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { applyLook, readLook } from './lib/look';

/**
 * 浏览器预览（ui/ 目录下 `npm run dev`）的装配点：没有 Tauri 运行时就把假后端
 * 装进 `lib/ipc/transport`，让所有 `ipc.*` 与事件订阅照常工作。
 *
 * 用动态 import 而不是顶层静态引入：mock 在 Tauri 构建里是独立 chunk，
 * 真宿主永不加载，也不会把 20KB 假数据塞进主 bundle。
 *
 * `?failCmd=library_list` 可注入一条命令失败，用来手验错误条渲染。
 * `?beets=absent` 让 `beets_status` 报"未安装"，用来演练整理库段的收拢形态
 * （docs/35 P2-11：这条路径以前只有肉眼验过）。
 */
if (!('__TAURI_INTERNALS__' in window)) {
  const { installMock } = await import('./lib/ipc/mock');
  const params = new URLSearchParams(window.location.search);
  const failCmd = params.get('failCmd');
  installMock({
    ...(failCmd ? { failCommands: [failCmd] } : {}),
    ...(params.get('beets') === 'absent' ? { beetsFound: false } : {}),
  });
}

const target = document.getElementById('app');
if (!target) throw new Error('#app not found');

/**
 * 首帧外观：**必须早于 mount**。
 *
 * 主题真值在 SQLite，`bootstrap()` 之后才到；这里读的是 `App` 主题 effect
 * 上次写下的 localStorage 镜像（`lib/look.ts` 有完整理由）。三级回退：
 * 镜像 → `__AIYA_LOOK__` → 默认深色。
 */
applyLook(readLook());

const app = mount(App, { target });

export default app;
