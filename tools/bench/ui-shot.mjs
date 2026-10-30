/**
 * UI 布局核对截图（P0 用，不属于发布链路）。
 *
 * 为什么走浏览器预览而不是 Tauri 窗口：这台机器上应用窗口截图被安全策略拦
 * （见 tools/cdp-probe.mjs 的说明），而预览跑的是同一份 UI 代码 + mock 后端，
 * 布局/文案/置灰这些"看得见"的东西完全一致。
 *
 * 前提：`npm run dev`（或 `npm run dev:ui`）已经起在 1420。
 */
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');

const BASE = process.env.AIYA_PREVIEW ?? 'http://localhost:1420/';
await mkdir('tools/bench/shots', { recursive: true });

const browser = await chromium.launch({ channel: 'msedge', headless: false });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(String(e.message ?? e)));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

await page.goto(BASE, { waitUntil: 'networkidle' });
await page.waitForTimeout(1200);

async function shot(name) {
  await page.screenshot({ path: `tools/bench/shots/${name}.png` });
  console.log('shot', name);
}

await shot('p0-home');

// 设置页：侧栏底部的「设置」
const settingsBtn = page.getByRole('button', { name: '设置', exact: true }).first();
if (await settingsBtn.count()) {
  await settingsBtn.click();
  await page.waitForTimeout(700);
  await shot('p0-settings');

  // 关掉「全局 3D 效果」：下面四项应当置灰（docs/46 §6.4）
  const glSwitch = page.getByRole('switch', { name: '全局 3D 效果' });
  if (await glSwitch.count()) {
    await glSwitch.click();
    await page.waitForTimeout(500);
    await shot('p0-settings-gl-off');
    const bgSwitch = page.getByRole('switch', { name: '背景动态频谱' });
    console.log('背景频谱禁用:', await bgSwitch.isDisabled().catch(() => 'n/a'));
    await glSwitch.click(); // 还原，别把设置留在测试状态
    await page.waitForTimeout(300);
  } else {
    console.log('未找到「全局 3D 效果」开关');
  }
}

// 正在播放页：播放条左侧的歌曲区域（title 里带「正在播放」或是 .player-song）
const candidates = [
  page.locator('.player-song').first(),
  page.locator('[title*="正在播放"]').first(),
  page.locator('.player-now').first(),
];
let opened = false;
for (const c of candidates) {
  if (await c.count()) {
    await c.click();
    await page.waitForTimeout(900);
    opened = true;
    break;
  }
}
if (opened) {
  await shot('p0-nowplaying');
  const cls = await page.locator('.app').getAttribute('class');
  console.log('app class:', cls);
  const sidebar = page.locator('.sidebar');
  console.log('sidebar visible:', await sidebar.isVisible().catch(() => 'n/a'));
  console.log('sidebar box:', JSON.stringify(await sidebar.boundingBox().catch(() => null)));
} else {
  console.log('未找到正在播放入口');
}

console.log('errors:', errors.length ? errors.slice(0, 5) : 'none');
await browser.close();
