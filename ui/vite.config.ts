import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

/**
 * AIYA UI（Tauri 2 WebView 前端）。
 *
 * 没有路径别名：契约类型**全部**来自 Rust 生成物（`src/lib/ipc/generated/`），
 * 前端自己的领域类型在 `src/lib/`。旧的 `@aiya/contracts` 别名（指向
 * `ui/contracts/src` 的 vendored 副本）在批次 4 随该目录一起删除。
 *
 * 端口 1420 是 Tauri devUrl 的约定值（tauri.conf.json 保持一致）。
 */
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // top-level await（main.ts 里动态装配预览后端）需要 esnext 级别；
    // chrome110 = WebView2 的保守下限，两者交集下 TLA 是安全的
    target: 'chrome110',
    outDir: 'dist',
  },
});
