/**
 * 验证 P1 流动渐变层（docs/46 §3.1）。
 *
 * 三轮，每轮都是"能不能自己证明自己错了"的否定式检查：
 *
 *   A 正常（硬件 GL）  —— 画布存在；开着时背景在缓慢变化；关掉后**完全静止**
 *   B reduced-motion   —— 画布根本不该创建（animations 要为 0，不是"动得慢一点"）
 *   C 软件光栅         —— 不建 WebGL 上下文、不崩、静态渐变仍在
 *
 * 用法：node tools/bench/verify-flow.mjs
 * 前提：`npm run dev` 在跑（真窗口 headed 才有硬件 GL；headless 会退到 SwiftShader）
 */
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { mkdir } from 'node:fs/promises';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');

const URL = 'http://localhost:1420/';
const GAP_MS = 5000;
const results = [];

await mkdir('tools/bench/shots', { recursive: true });

async function open(opts = {}) {
  const browser = await chromium.launch({ channel: 'msedge', headless: false, args: opts.args ?? [] });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    reducedMotion: opts.reducedMotion,
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.goto(URL, { waitUntil: 'networkidle' });
  await page.waitForTimeout(1400);
  return { browser, page, errors };
}

async function setSwitch(page, name, want) {
  const sw = page.getByRole('switch', { name });
  if ((await sw.getAttribute('aria-checked')) !== want) {
    await sw.click();
    await page.waitForTimeout(700);
  }
  return sw.getAttribute('aria-checked');
}

/** 在页面里解码两张 PNG 求像素差（Node 侧没有解码器） */
async function diff(page, a, b, rect) {
  return page.evaluate(
    async ({ a, b, rect }) => {
      const load = (b64) =>
        new Promise((res, rej) => {
          const img = new Image();
          img.onload = () => res(img);
          img.onerror = rej;
          img.src = 'data:image/png;base64,' + b64;
        });
      const [ia, ib] = await Promise.all([load(a), load(b)]);
      const cv = new OffscreenCanvas(ia.width, ia.height);
      const cx = cv.getContext('2d');
      const grab = (img) => {
        cx.clearRect(0, 0, cv.width, cv.height);
        cx.drawImage(img, 0, 0);
        return cx.getImageData(rect.x, rect.y, rect.w, rect.h).data;
      };
      const da = grab(ia);
      const db = grab(ib);
      let sum = 0;
      let max = 0;
      for (let i = 0; i < da.length; i += 4) {
        const d = Math.abs(da[i] - db[i]) + Math.abs(da[i + 1] - db[i + 1]) + Math.abs(da[i + 2] - db[i + 2]);
        sum += d;
        if (d > max) max = d;
      }
      return { mean: sum / (da.length / 4) / 3, max };
    },
    { a: a.toString('base64'), b: b.toString('base64'), rect },
  );
}

// 右边缘一条竖带：远离侧栏、列表与播放条，只有背景
const RECT = { x: 1150, y: 160, w: 100, h: 420 };

async function pair(page, label) {
  const a = await page.screenshot();
  await page.waitForTimeout(GAP_MS);
  const b = await page.screenshot();
  const r = await diff(page, a, b, RECT);
  console.log(`  ${label}: meanΔ=${r.mean.toFixed(3)} maxΔ=${r.max}`);
  return r;
}

// ---------------- A 正常路径 ----------------
console.log('\n== A 硬件 GL ==');
{
  const { browser, page, errors } = await open();
  const cap = await page.evaluate(async () => {
    const { detectCapability } = await import('/src/lib/render/capability.ts');
    return detectCapability();
  });
  console.log('  capability:', JSON.stringify(cap));

  await page.getByRole('button', { name: '设置', exact: true }).first().click();
  await page.waitForTimeout(600);
  await setSwitch(page, '背景动态频谱', 'true');

  const onCanvas = await page.locator('canvas.ambient-flow').count();
  const tickers = await page.evaluate(async () => {
    const m = await import('/src/lib/render/scheduler.ts');
    return m.renderTickCount();
  });
  console.log(`  canvas=${onCanvas} tickers=${tickers}`);

  await page.screenshot({ path: 'tools/bench/shots/p1-settings-on.png' });
  const on = await pair(page, 'flow ON ');

  await setSwitch(page, '背景动态频谱', 'false');
  await page.waitForTimeout(1200);
  const offCanvas = await page.locator('canvas.ambient-flow').count();
  await page.screenshot({ path: 'tools/bench/shots/p1-settings-off.png' });
  const off = await pair(page, 'flow OFF');

  results.push(['A 画布存在（开着）', onCanvas === 1, `count=${onCanvas}`]);
  results.push(['A 画布销毁（关着）', offCanvas === 0, `count=${offCanvas}`]);
  results.push(['A 开着时有运动', on.mean > 0.05, `meanΔ=${on.mean.toFixed(3)}`]);
  results.push(['A 关着时完全静止', off.mean < 0.01, `meanΔ=${off.mean.toFixed(3)}`]);
  results.push(['A 无 console error', errors.length === 0, errors.slice(0, 2).join('|')]);
  await browser.close();
}

// ---------------- B reduced-motion ----------------
console.log('\n== B prefers-reduced-motion: reduce ==');
{
  const { browser, page, errors } = await open({ reducedMotion: 'reduce' });
  await page.getByRole('button', { name: '设置', exact: true }).first().click();
  await page.waitForTimeout(600);
  await setSwitch(page, '背景动态频谱', 'true');
  const count = await page.locator('canvas.ambient-flow').count();
  console.log(`  canvas=${count}`);
  results.push(['B reduced-motion 不建画布', count === 0, `count=${count}`]);
  results.push(['B 无 console error', errors.length === 0, errors.slice(0, 2).join('|')]);
  await browser.close();
}

// ---------------- C 软件光栅 ----------------
console.log('\n== C 软件光栅（--disable-gpu） ==');
{
  const { browser, page, errors } = await open({ args: ['--disable-gpu', '--use-angle=swiftshader'] });
  const cap = await page.evaluate(async () => {
    const { detectCapability } = await import('/src/lib/render/capability.ts');
    return detectCapability();
  });
  console.log('  capability:', JSON.stringify(cap));
  await page.getByRole('button', { name: '设置', exact: true }).first().click();
  await page.waitForTimeout(600);
  await setSwitch(page, '背景动态频谱', 'true');
  await page.waitForTimeout(600);
  const count = await page.locator('canvas.ambient-flow').count();
  const bgVisible = await page.locator('.ambient-art').isVisible();
  console.log(`  canvas=${count} css-gradient-visible=${bgVisible}`);
  await page.screenshot({ path: 'tools/bench/shots/p1-software.png' });
  results.push(['C 软件光栅不建画布', count === 0, `count=${count}`]);
  results.push(['C 静态渐变仍在', bgVisible, `visible=${bgVisible}`]);
  results.push(['C 无 console error', errors.length === 0, errors.slice(0, 2).join('|')]);
  await browser.close();
}

console.log('\n--- 结论 ---');
let failed = 0;
for (const [name, ok, detail] of results) {
  if (!ok) failed += 1;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  (${detail})`);
}
console.log(failed === 0 ? '\n全部通过' : `\n${failed} 项未通过`);
process.exitCode = failed === 0 ? 0 : 1;
