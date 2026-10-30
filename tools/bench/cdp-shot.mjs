/**
 * 真窗口（Tauri / WebView2）截图与状态读取，走 CDP。
 *
 * 为什么单独写一个：这台机器上**操作系统级**的窗口截图被安全策略拦住，
 * 但 CDP 的 `Page.captureScreenshot` 是浏览器内部合成结果的回读，
 * 不经过 OS 截屏接口，能拿到真实窗口的内容。
 *
 * 用途：验证 WebGL 层在无边框 + 透明合成窗口里的表现——这是 docs/46 §2
 * 明确标为"未验证"的一项，浏览器预览通过不代表真窗口通过。
 *
 * 用法：
 *   1) WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run dev
 *   2) node tools/bench/cdp-shot.mjs [输出路径]
 */
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';

const OUT = process.argv[2] ?? 'tools/bench/shots/tauri-window.png';
const targets = await (await fetch('http://127.0.0.1:9222/json')).json();
const page = targets.find((t) => t.type === 'page' && /^https?:/.test(t.url ?? ''));
if (!page) {
  console.error('没有可用的页面目标；先带 --remote-debugging-port=9222 启动 dev');
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});

function send(method, params = {}) {
  const msgId = ++id;
  ws.send(JSON.stringify({ id: msgId, method, params }));
  return new Promise((resolve) => pending.set(msgId, resolve));
}

await new Promise((resolve) => ws.addEventListener('open', resolve, { once: true }));

const state = await send('Runtime.evaluate', {
  expression: `(() => {
    const c = document.querySelector('canvas.ambient-flow');
    const g = c ? c.getContext('webgl2') : null;
    return {
      url: location.href,
      viewport: [innerWidth, innerHeight],
      dpr: devicePixelRatio,
      theme: document.documentElement.dataset.theme,
      flowCanvas: document.querySelectorAll('canvas.ambient-flow').length,
      flowBuffer: g ? [g.drawingBufferWidth, g.drawingBufferHeight] : null,
      ridgeCanvas: document.querySelectorAll('canvas.ambient-spectrum').length,
      npOpen: document.querySelector('.app')?.className ?? '',
    };
  })()`,
  returnByValue: true,
});
console.log('state:', JSON.stringify(state.result?.result?.value));

const shot = await send('Page.captureScreenshot', { format: 'png' });
const data = shot.result?.data;
if (!data) {
  console.error('截图失败:', JSON.stringify(shot.error ?? shot));
  process.exit(1);
}
await mkdir(dirname(OUT), { recursive: true });
await writeFile(OUT, Buffer.from(data, 'base64'));
console.log('saved:', OUT);
ws.close();
