/**
 * UI 运行时探针（默认探结构 / `--actions` 探交互）。
 *
 * 为什么需要它：Tauri 的应用窗口截图被这台机器的安全策略拦，
 * 所以"界面没坏"没法用截图证明。这个脚本走 WebView2 / 浏览器的远程调试
 * 端口（CDP），在真窗口里读 DOM、抓 console.error 与未捕获异常 ——
 * 既不依赖截图权限，也不需要给应用加测试钩子。
 *
 * 用法：
 *   1) Tauri 真窗口：
 *      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run dev
 *   2) 浏览器预览（`npm run dev:ui` 起的 1420 端口）：
 *      msedge --headless=new --remote-debugging-port=9222 http://localhost:1420/
 *   然后：
 *      npm run probe:ui                 # 结构 + 错误
 *      npm run probe:ui -- --actions    # 面板开关等交互序列
 *
 * 判定：有未捕获异常、或除 favicon 之外的 console.error → 退出码 1。
 *
 * 实现要点：
 * - **选目标**：浏览器会带 `edge://`、扩展后台页等杂项目标，盲取 [0] 会连到
 *   同步确认弹窗上。只认 http(s) 页面（Tauri 是 http://localhost:1420 或
 *   http://tauri.localhost，同样命中）。
 * - 先 `Runtime.enable`/`Log.enable`/`Page.enable` 再 `Page.reload`。
 *   探针连上时页面早已加载完，不 reload 就抓不到挂载期的异常 ——
 *   而拆壳/换数据层最容易出问题的正是挂载期。
 */
const ENDPOINT = 'http://127.0.0.1:9222/json';
const ACTIONS = process.argv.includes('--actions');
const PLAY = process.argv.includes('--play');
/** 可选：`--url=<子串>` 精确挑页面（多开时用） */
const URL_FILTER = process.argv.find((a) => a.startsWith('--url='))?.slice('--url='.length) ?? null;

const pages = (await (await fetch(ENDPOINT)).json()).filter((t) => t.type === 'page');
const targets = pages.filter(
  (t) =>
    /^https?:/.test(t.url ?? '') && (URL_FILTER === null || (t.url ?? '').includes(URL_FILTER)),
);
if (targets.length === 0) {
  console.error('没有 http(s) page 目标：确认 dev 启动时带了 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS');
  console.error(`现有 page 目标：${JSON.stringify(pages.map((t) => t.url))}`);
  process.exit(2);
}

const ws = new WebSocket(targets[0].webSocketDebuggerUrl);
let seq = 0;
const pending = new Map();
const consoleEvents = [];
const exceptions = [];
const failedResponses = [];
let loadEvents = 0;

const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++seq;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });

await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg.result);
    pending.delete(msg.id);
    return;
  }
  switch (msg.method) {
    case 'Runtime.consoleAPICalled': {
      const { type, args } = msg.params;
      consoleEvents.push({
        type,
        text: args.map((a) => a.value ?? a.description ?? a.type).join(' '),
      });
      break;
    }
    case 'Runtime.exceptionThrown': {
      const d = msg.params.exceptionDetails;
      exceptions.push(d.exception?.description ?? d.text);
      break;
    }
    case 'Log.entryAdded': {
      const e = msg.params.entry;
      consoleEvents.push({ type: e.level, text: `${e.source}: ${e.text}` });
      break;
    }
    case 'Network.responseReceived': {
      const r = msg.params.response;
      if (r.status >= 400) failedResponses.push(`${r.status} ${r.url}`);
      break;
    }
    case 'Page.loadEventFired':
      loadEvents += 1;
      break;
  }
};

const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (r?.exceptionDetails) {
    return { __evalError: r.exceptionDetails.exception?.description ?? r.exceptionDetails.text };
  }
  return r?.result?.value;
};

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');
await send('Network.enable');

// reload 之前的事件属于上一次会话，丢掉
consoleEvents.length = 0;
exceptions.length = 0;
failedResponses.length = 0;
await send('Page.reload', { ignoreCache: true });
for (let i = 0; i < 60 && loadEvents === 0; i += 1) await wait(100);
// 启动异步链：bootstrap → loadStats → 首屏 list
await wait(3000);

const STRUCTURE = `(() => {
  const app = document.querySelector('.app');
  const q = (s) => document.querySelectorAll(s).length;
  /** 真实数据行：排除「失败」「骨架屏」两种占位行 */
  const rows = [...document.querySelectorAll('.main .list .row')]
    .filter((el) => !el.classList.contains('failed') && !el.classList.contains('skeleton'));
  return {
    appChildren: app ? [...app.children].map((el) => el.tagName.toLowerCase() + '.' + (el.className || '(none)')) : null,
    mainIsDirectChildOfApp: !!document.querySelector('.app > main.main'),
    playerBar: q('.app > footer.player') > 0,
    sidebarNavItems: q('.nav-item'),
    sidebarPlaylists: q('.sidebar .playlist-item'),
    trackRows: rows.length,
    firstRowTitle: rows[0]?.querySelector('.cell.title')?.textContent?.trim() ?? null,
    playerTrackTitle: document.querySelector('.player .track-title, .player .title')?.textContent?.trim() ?? null,
    queuePanel: q('.queue-panel') > 0,
    lyricsPanel: q('.lyrics-panel') > 0,
    spectrumPanel: q('.spectrum-panel') > 0,
    batchBar: q('.batch-bar') > 0,
    toast: q('.toast') > 0,
    theme: document.documentElement.dataset.theme ?? null,
    density: document.documentElement.dataset.density ?? null,
    emptyStrong: document.querySelector('.main .empty strong')?.textContent ?? null,
    errorBar: document.querySelector('.main .error')?.textContent?.trim() ?? null,
    accent: document.documentElement.style.getPropertyValue('--accent') || null,
  };
})()`;

console.log('=== 结构 ===');
console.log(JSON.stringify(await evaluate(STRUCTURE), null, 2));

if (ACTIONS) {
  const snap = () =>
    evaluate(`(() => {
      const app = document.querySelector('.app');
      return {
        sideOpen: app?.classList.contains('side-open') ?? null,
        queue: !!document.querySelector('.queue-panel'),
        lyrics: !!document.querySelector('.lyrics-panel'),
      };
    })()`);
  const click = async (label) => {
    const r = await evaluate(
      `(() => { const el = document.querySelector('footer.player button[aria-label="${label}"]');
        if (!el) return 'NOT_FOUND'; el.click(); return 'CLICKED'; })()`,
    );
    await wait(350);
    return { label, clicked: r, state: await snap() };
  };

  console.log('\n=== 交互序列（播放条 → 侧面板）===');
  for (const label of ['播放队列', '歌词', '播放队列', '播放队列']) {
    // 队列开 → 切歌词 → 切回队列 → 关队列
    console.log(JSON.stringify(await click(label)));
  }
}

/**
 * `--play`：点第一行 → 读播放条。
 *
 * 这条链路同时验证四件事：列表行点击 → `ipc.player.playQuery` →
 * mock/引擎的 `player:state` 事件 → store 更新 → 播放条渲染。
 * 预览模式下它证明事件注入（`transport().listen`）是通的；Tauri 下同理。
 */
if (PLAY) {
  console.log('\n=== 播放链路（点第一行）===');
  const clicked = await evaluate(`(() => {
    const row = [...document.querySelectorAll('.main .list .row')]
      .find((el) => !el.classList.contains('failed') && !el.classList.contains('skeleton'));
    if (!row) return 'NO_ROW';
    row.click();
    return 'CLICKED';
  })()`);
  await wait(600);
  const bar = await evaluate(`(() => {
    const p = document.querySelector('footer.player');
    const btn = p?.querySelector('button.play');
    return {
      title: p?.querySelector('.now .title')?.textContent?.trim() ?? null,
      artist: p?.querySelector('.now .artist')?.textContent?.trim() ?? null,
      badge: p?.querySelector('.fmt-badge')?.textContent?.trim() ?? null,
      spec: p?.querySelector('.fmt-spec')?.textContent?.trim() ?? null,
      tooltip: p?.querySelector('.fmt-badge')?.getAttribute('title') ?? null,
      transportLabel: btn?.getAttribute('aria-label') ?? null,
      paused: btn?.classList.contains('loading') ?? null,
    };
  })()`);
  console.log(JSON.stringify({ clicked, ...bar }, null, 2));
}

console.log('\n=== console error / warning ===');
const bad = consoleEvents.filter((e) => e.type === 'error' || e.type === 'warning');
if (bad.length === 0) console.log('(无)');
else for (const e of bad) console.log(`[${e.type}] ${e.text}`);

console.log('\n=== 4xx / 5xx 响应 ===');
const realFailures = failedResponses.filter((f) => !f.endsWith('/favicon.ico'));
if (realFailures.length === 0) console.log('(无，favicon 404 属 dev server 既有现象)');
else for (const f of realFailures) console.log(f);

console.log('\n=== 未捕获异常 ===');
if (exceptions.length === 0) console.log('(无)');
else for (const e of exceptions) console.log(e);

ws.close();
// 「Failed to load resource」这类 console 错误没有 URL，真实原因由上面的
// Network 响应判定（已排除 favicon），这里不再重复计一次失败。
const consoleFailures = bad.filter(
  (e) => e.type === 'error' && !e.text.includes('Failed to load resource'),
);
const failed = exceptions.length > 0 || realFailures.length > 0 || consoleFailures.length > 0;
process.exit(failed ? 1 : 0);
