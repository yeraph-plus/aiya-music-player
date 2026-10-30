/**
 * 频谱渲染性能基准采集（调研用，不属于发布链路）。
 *
 * 为什么需要它：三个待定方向（three.js 重绘山形 / 真毛玻璃 / 流动渐变）
 * 的取舍都取决于同一个问题——每帧多花多少毫秒。headless 浏览器走
 * SwiftShader 软件光栅，数据没有参考价值，所以这里用**系统 Edge 的
 * headed 窗口**（与 WebView2 同引擎）跑真实 GPU 路径。
 *
 * 用法：node tools/bench/collect.mjs
 * 输出：每场景一行的 JSON（fps / p50 / p95 / 掉帧数 / 长任务），
 *       以及一组汇总（同一 DPR 下的场景对比）。
 */
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { extname, join, normalize, resolve } from 'node:path';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');

const ROOT = resolve('.');
const PORT = 8137;
const argOf = (name) => process.argv.find((a) => a.startsWith(`--${name}=`))?.split('=')[1] ?? null;
const SCENES = argOf('scenes') ? argOf('scenes').split(',') : ['off', 'off-glass', 'canvas', 'canvas-glass', 'webgl', 'webgl-glass'];
const DPRS = argOf('dprs') ? argOf('dprs').split(',').map(Number) : [1, 2];
const VW = Number(argOf('vw') ?? 1280);
const VH = Number(argOf('vh') ?? 800);

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
};

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://127.0.0.1:${PORT}`);
    const path = normalize(join(ROOT, decodeURIComponent(url.pathname)));
    if (!path.startsWith(ROOT)) {
      res.writeHead(403).end();
      return;
    }
    const body = await readFile(path);
    res.writeHead(200, { 'content-type': MIME[extname(path)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
});

await new Promise((r) => server.listen(PORT, '127.0.0.1', r));

// 解除 vsync 上限：否则所有场景都贴在 60fps，差别全被掩盖
const UNLOCK = !process.argv.includes('--vsync');
// --soft：强制 SwiftShader 软件光栅，模拟无 GPU 加速的机器（远程桌面 / 老核显）
const SOFT = process.argv.includes('--soft');
const browser = await chromium.launch({
  channel: 'msedge',
  headless: false,
  args: [
    ...(UNLOCK ? ['--disable-gpu-vsync', '--disable-frame-rate-limit'] : []),
    ...(SOFT ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : []),
  ],
});
const rows = [];

for (const dpr of DPRS) {
  const context = await browser.newContext({
    viewport: { width: VW, height: VH },
    deviceScaleFactor: dpr,
  });
  for (const scene of SCENES) {
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e.message ?? e)));
    page.on('console', (m) => {
      if (m.type() === 'error') errors.push(m.text());
    });
    await page.goto(`http://127.0.0.1:${PORT}/tools/bench/spectrum-bench.html?s=${scene}&dpr=${dpr}&ms=${argOf("ms") ?? 5000}`);
    let out;
    try {
      out = await page.waitForFunction(() => window.__BENCH__, null, { timeout: Number(argOf("timeout") ?? 20000) }).then((h) => h.jsonValue());
    } catch {
      out = { scene, dpr, error: 'timeout', errors };
    }
    if (errors.length) out.errors = errors.slice(0, 3);
    rows.push(out);
    console.log(JSON.stringify(out));
    await page.close();
  }
  await context.close();
}

await browser.close();
server.close();
