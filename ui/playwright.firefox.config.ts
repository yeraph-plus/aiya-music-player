import { defineConfig } from '@playwright/test';

/**
 * Firefox 真机矩阵配置（docs/66 W6）：同一套 mock 套件跑在 Playwright 的
 * bundled Firefox 上——此前 e2e 只在 Chromium 系（系统 Edge）上跑过，
 * Firefox 是真机矩阵清单里唯一能在 Windows 上补测的浏览器（Safari 需要
 * macOS/iOS 设备，无法在本机回归）。需要先 `npx playwright install firefox`。
 */
export default defineConfig({
  testDir: './e2e',
  testIgnore: '**/e2e/http/**',
  timeout: 30_000,
  retries: 0,
  workers: 1,
  use: {
    browserName: 'firefox',
    headless: true,
    baseURL: 'http://localhost:4173',
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    port: 4173,
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
