import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

/**
 * AIYA UI（Tauri 2 WebView 前端）。
 *
 * - `@aiya/contracts` 指到本仓库内 vendored 副本（ui/contracts/src），
 *   组件里的 import 一个都不用改 —— 与旧 Electron 工程同构。
 * - 端口 1420 是 Tauri devUrl 的约定值（tauri.conf.json 保持一致）。
 */
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  resolve: {
    alias: {
      '@aiya/contracts': resolve(import.meta.dirname, 'contracts/src/index.ts'),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: 'chrome110',
    outDir: 'dist',
  },
});
