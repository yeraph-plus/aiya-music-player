import { defineConfig } from '@playwright/test';

/** 源码预览回归可复用正在运行的 Vite，不读取或生成 dist。 */
export default defineConfig({
  testDir: './e2e',
  testIgnore: '**/http/**',
  timeout: 30_000,
  retries: 0,
  workers: 1,
  use: {
    channel: 'msedge', headless: true,
    baseURL: 'http://localhost:1420', viewport: { width: 1280, height: 800 },
  },
  projects: [
    { name: 'regression', outputDir: './test-results/dev-regression', testIgnore: ['**/http/**', '**/baseline.spec.ts'] },
    { name: 'baseline', outputDir: './test-results/ui-baseline', testMatch: '**/dev/baseline.spec.ts' },
  ],
  webServer: {
    command: 'npx vite --port 1420 --strictPort',
    url: 'http://localhost:1420',
    reuseExistingServer: true,
    timeout: 30_000,
  },
});
