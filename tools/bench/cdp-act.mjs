/**
 * 真窗口（Tauri / WebView2）动作 + 截图：走 Playwright 的 `connectOverCDP`。
 *
 * 为什么不用裸 CDP：`Page.captureScreenshot` 在"页面持续动画 + 长时间持有会话"
 * 的情况下会悬住不返回（实测两次都卡在同一行，socket 被对端关掉、Promise 永不
 * settle）。Playwright 的 `page.screenshot()` 走同一通道但有超时与重试，稳定得多。
 *
 * 用法：
 *   1) WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run dev
 *   2) node tools/bench/cdp-act.mjs --glow=平衡 --out=tools/bench/shots/x.png [--np=1]
 *
 * 说明：`--glow` 需要在设置页点段选，脚本会自己回退到设置页再进正在播放页。
 */
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');

const argOf = (name, fallback) => {
  const hit = process.argv.find((a) => a.startsWith(`--${name}=`));
  return hit ? hit.slice(name.length + 3) : fallback;
};

const out = argOf('out', 'tools/bench/shots/tauri.png');
const glow = argOf('glow', null);
const toNp = argOf('np', '1') === '1';

const browser = await chromium.connectOverCDP('http://127.0.0.1:9222');
const context = browser.contexts()[0];
const page = context.pages().find((x) => /^https?:/.test(x.url()));
if (!page) {
  console.error('没找到页面目标');
  process.exit(1);
}

// 正在播放页会盖住侧栏，先退出去才点得到"设置"。
// 页内没有返回键（2026-10-30 撤了）：退出走播放条歌曲区那个开关（或顶栏返回）
if (await page.locator('.app.np-open').count()) {
  await page.locator('.player .now').first().click();
  await page.waitForTimeout(600);
}

if (glow) {
  await page.getByRole('button', { name: '设置', exact: true }).first().click();
  await page.waitForTimeout(500);
  const radio = page.getByRole('radiogroup', { name: '背景频谱光效' }).getByRole('radio', { name: glow, exact: true });
  await radio.click();
  await page.waitForTimeout(600);
  console.log(`glow=${glow} checked=${await radio.getAttribute('aria-checked')}`);
}

if (toNp) {
  await page.locator('.player .now').first().click();
  await page.waitForTimeout(1600);
}

const state = await page.evaluate(() => ({
  app: document.querySelector('.app')?.className ?? '',
  flow: document.querySelectorAll('canvas.ambient-flow').length,
  ridge2d: document.querySelectorAll('canvas.ambient-spectrum').length,
  viewport: [innerWidth, innerHeight],
}));
console.log('state:', JSON.stringify(state));

await mkdir(dirname(out), { recursive: true });
await page.screenshot({ path: out });
console.log('saved:', out);

// 不调 browser.close()：CDP 连接上的 close 可能把宿主窗口一起关掉，
// 断开 socket 即可
process.exit(0);
