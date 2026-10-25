import { defineConfig } from '@playwright/test';

/**
 * E2E 冒烟配置（docs/18 §9 大件 C）。
 *
 * - 被测对象是**浏览器预览里的 mock 后端**（main.ts：无 Tauri 运行时自动装
 *   mock），不起 Tauri、不碰 sidecar。冒烟覆盖 UI 装配与关键交互链路，
 *   真引擎行为归 Rust 单测与真窗口探针（tools/cdp-probe.mjs）。
 * - 用系统 Edge（channel: 'msedge'），不下载 Playwright 浏览器：
 *   安装时 PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1，省 150MB 且版本跟系统走。
 * - webServer 用 `vite preview`（吃 dist 产物），比 dev server 稳定、
 *   无 HMR 干扰；测试前跑 `npm run build`（见 package.json 的 test:e2e）。
 */
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  retries: 0,
  workers: 1, // mock 后端是单实例内存态，禁并发
  use: {
    channel: 'msedge',
    headless: true,
    baseURL: 'http://localhost:4173',
    // 页面里任何 console.error / pageerror 都算失败（在 spec 里断言）
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    port: 4173,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
