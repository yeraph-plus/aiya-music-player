import { defineConfig } from '@playwright/test';

/**
 * http 形态 e2e（docs/66 §11）：被测对象是**真 aiya-server**（axum + SSE +
 * Range + 登录门），不是 mock。浏览器跑的是 Web 宿主的完整链路——
 * transport 走 HTTP、播放引擎是 `<audio>`、频谱走 AnalyserNode。
 *
 * 服务端由 `tools/http-e2e-server.mjs` 拉起：cargo 增量构建 aiya-server →
 * 拷贝 fixtures 到临时曲库 → 带口令启动（8799）→ 就绪探测。数据落在临时
 * 目录，跑完即弃。
 */
export default defineConfig({
  testDir: './e2e/http',
  timeout: 30_000,
  retries: 0,
  workers: 1, // 单实例服务端 + 共享曲库，禁并发
  use: {
    channel: 'msedge',
    headless: true,
    baseURL: 'http://127.0.0.1:8799',
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: 'node ../tools/http-e2e-server.mjs',
    port: 8799,
    reuseExistingServer: false,
    timeout: 240_000, // 首跑含 cargo 增量构建
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
