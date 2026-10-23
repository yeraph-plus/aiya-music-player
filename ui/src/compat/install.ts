import { installApi } from './api.js';

/**
 * compat 安装入口（index.html 中先于 main.ts 执行）。
 *
 * 模块按文档顺序求值，这里同步赋值 `window.api`，
 * main.ts 与任何组件都能在 mount 前拿到完整 AiyaApi。
 */
installApi();
