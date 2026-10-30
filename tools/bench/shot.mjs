// 视觉校验：确认 bench 的四个场景真的画出了东西（性能数字不能建立在空帧上）
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { extname, join, normalize, resolve } from 'node:path';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');
const ROOT = resolve('.');
const PORT = 8138;
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8' };
const server = createServer(async (req, res) => {
  try {
    const p = normalize(join(ROOT, decodeURIComponent(new URL(req.url, 'http://x').pathname)));
    const body = await readFile(p);
    res.writeHead(200, { 'content-type': MIME[extname(p)] ?? 'application/octet-stream' }).end(body);
  } catch { res.writeHead(404).end(); }
});
await new Promise((r) => server.listen(PORT, '127.0.0.1', r));
await mkdir('tools/bench/shots', { recursive: true });
const browser = await chromium.launch({ channel: 'msedge', headless: false });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
for (const s of ['canvas', 'canvas-glass', 'webgl', 'webgl-glass']) {
  const page = await ctx.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text()));
  await page.goto(`http://127.0.0.1:${PORT}/tools/bench/spectrum-bench.html?s=${s}&ms=600`);
  await page.waitForTimeout(2200);
  await page.screenshot({ path: `tools/bench/shots/${s}.png` });
  console.log(s, errs.filter((e) => !e.includes('404')).join(' | ') || 'ok');
  await page.close();
}
await browser.close(); server.close();
