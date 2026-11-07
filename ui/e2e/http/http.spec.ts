import { expect, test, type Page } from '@playwright/test';

/**
 * http 形态 e2e（docs/66 §11）：真 aiya-server + 浏览器引擎的完整链路。
 *
 * 与 mock 套件的分工：mock 锁 UI 装配与桌面形态，这里锁 Web 形态——
 * 登录闸首帧、首启种子根、浏览器解码灰标、`<audio>` 真播放、设置页
 * 隐身清单、会话刷新直进与双语。服务端由 webServer 钩子拉起
 * （tools/http-e2e-server.mjs，8799，口令 e2e-pass）。
 *
 * 断言尽量走结构与 evaluate（DOM 直查），少依赖文案——界面语言跟随
 * 服务端首启写库，测试机之间可能是 zh 或 en；需要文案的用例先点
 * 「中文/English」语言钮固定语向（语言钮文案是语文名，恒定）。
 */

const PASSWORD = 'e2e-pass';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.gate input[type="password"]').fill(PASSWORD);
  await page.locator('.gate button').click();
  await page.locator('nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
}

async function goToLibrary(page: Page): Promise<void> {
  await page.locator('aside button', { hasText: /曲库|Library/ }).click();
  await page.locator('.row').first().waitFor({ state: 'visible', timeout: 10_000 });
}

test('登录闸首帧渲染，口令进闸后应用可用', async ({ page }) => {
  await page.goto('/');
  // 首帧即闸（mount 前的 /auth/state 预检）：主界面不得先闪现
  await page.locator('.gate').waitFor({ state: 'visible', timeout: 10_000 });
  expect(await page.locator('nav').count()).toBe(0);
  await page.locator('.gate input[type="password"]').fill(PASSWORD);
  await page.locator('.gate button').click();
  await page.locator('nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
});

test('首启种子根已入库，浏览器解不了的容器行打灰', async ({ page }) => {
  await login(page);
  await goToLibrary(page);
  // fixtures 三支（mp3 / ogg / aiff）都在库里：bootstrap_scan 的种子根
  const rowCount = await page.locator('.row').count();
  expect(rowCount).toBeGreaterThanOrEqual(3);
  // AIFF 行打灰（Chromium 解不了），其余行不打灰——灰标只此一家
  const unsupported = await page.locator('.row.unsupported').count();
  expect(unsupported).toBe(1);
});

test('双击播放：浏览器引擎装载并真实出声', async ({ page }) => {
  await login(page);
  await goToLibrary(page);
  // 指定一支可播容器（AIFF 行点了会走报错，不选）；曲目无标签，标题=文件名
  await page.locator('.row', { hasText: 'tone-mp3' }).dblclick();
  // <audio> 真装载：src 指向 /audio/ 且确实在走表。轮询带回显诊断——
  // paused/readyState/error 的组合能区分"被 autoplay 拦"与"加载失败"
  const deadline = Date.now() + 15_000;
  let last: unknown = null;
  for (;;) {
    last = await page.evaluate(() => {
      const a = document.querySelector('audio');
      if (!a) return null;
      return {
        playing: !a.paused && a.currentTime > 0,
        paused: a.paused,
        t: a.currentTime,
        ready: a.readyState,
        err: a.error?.code ?? null,
        src: a.src.split('/').slice(-1)[0],
      };
    });
    if (last && (last as { playing: boolean }).playing) break;
    if (Date.now() > deadline) break;
    await page.waitForTimeout(400);
  }
  expect(last, JSON.stringify(last)).toMatchObject({ playing: true });
});

test('设置页 web 形态：桌面段落隐身 + webDecode 说明 + 路径手输', async ({ page }) => {
  await login(page);
  await page.locator('aside button', { hasText: /设置|Settings/ }).click();
  // 固定中文语向（语言钮是语文名，两种语言下都这么叫）
  await page.getByRole('radio', { name: '中文' }).click();
  // 语言切换是服务端设置 + 本地即时生效：等中文文案可见（自动重试防慢机）
  await expect(page.getByText('最近播放保留')).toBeVisible();
  const shape = await page.evaluate(() => {
    const text = document.body.innerText;
    const labels = [...document.querySelectorAll('button[aria-label]')].map((b) =>
      b.getAttribute('aria-label'),
    );
    return {
      // 桌面专属段落不渲染
      outputHidden: !text.includes('输出模式') && !text.includes('Output mode'),
      sleepHidden: !text.includes('阻止系统休眠') && !text.includes('Prevent sleep'),
      trayHidden: !text.includes('关闭到托盘') && !text.includes('Close to tray'),
      // 窗控三键不渲染
      windowChromeHidden: !labels.some((l) => /最小化|Minimize/.test(l ?? '')),
      // EQ 触发键不渲染（队列键仍在）
      eqHidden: !labels.some((l) => /均衡器|EQ \(/.test(l ?? '')),
      // web 静态说明与路径手输在
      webDecode: text.includes('由浏览器解码播放') || text.includes('Decoded and played by the browser'),
      rootInput: !!document.querySelector<HTMLInputElement>('.roots-actions .root-input'),
    };
  });
  expect(shape).toEqual({
    outputHidden: true,
    sleepHidden: true,
    trayHidden: true,
    windowChromeHidden: true,
    eqHidden: true,
    webDecode: true,
    rootInput: true,
  });
});

test('会话刷新直进 + 双语抽查（English）', async ({ page }) => {
  await login(page);
  // 有效会话刷新：预检带凭据，直接进应用不再落闸
  await page.reload();
  await page.locator('nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
  // 双语抽查：切 English 后设置页曲库组标题是英文
  await page.locator('aside button', { hasText: /设置|Settings/ }).click();
  await page.getByRole('radio', { name: 'English' }).click();
  await expect(page.getByText('Library folders')).toBeVisible();
});

test('推流频道：建频道进列表 + 站单卡片进收听 tab + 频道删除', async ({ page }) => {
  await login(page);
  // 曲库导航到直播页：双 tab（caps.streaming web 形态才渲染「我的频道」）
  await page.locator('aside button', { hasText: /直播|Live/ }).click();
  await expect(page.locator('.text-tab', { hasText: /我的频道|My stations/ })).toBeVisible();
  await page.locator('.text-tab', { hasText: /我的频道|My stations/ }).click();
  await expect(page.getByText(/还没有频道|No stations yet/)).toBeVisible();

  // 新建频道（public）：保存后进详情（歌单/节目表编排入口），返回列表见卡
  await page.getByRole('button', { name: /新建频道|New station/ }).click();
  await page.getByPlaceholder(/如「深夜 FM」|Midnight FM/).fill('E2E Radio');
  await page.locator('select').first().selectOption('public');
  await page.getByRole('button', { name: /保存|Save/ }).click();
  await expect(page.getByRole('heading', { name: 'E2E Radio' })).toBeVisible();
  await page.locator('.st-back').click();
  await expect(page.locator('.st-card-name', { hasText: 'E2E Radio' })).toBeVisible();

  // 站单下发：切回收听 tab，service 卡片自动出现（hlsUrl 合并进 liveRecent）
  await page.locator('.text-tab', { hasText: /收听|Listen/ }).click();
  await expect(page.getByText('E2E Radio').first()).toBeVisible();

  // 元数据编辑：改 visibility 保存 → 列表卡 meta 变「公开」
  await page.locator('.text-tab', { hasText: /我的频道|My stations/ }).click();
  await page.locator('.st-card-name', { hasText: 'E2E Radio' }).click();
  await page.getByRole('button', { name: /编辑|Edit/ }).click();
  await page.locator('select').first().selectOption('private');
  await page.getByRole('button', { name: /保存|Save/ }).click();
  await expect(page.getByRole('heading', { name: 'E2E Radio' })).toBeVisible();
  await page.locator('.st-back').click();
  await expect(page.locator('.st-card', { hasText: 'E2E Radio' })).toContainText(/私人|Private/);

  // 清理：删除频道（confirm 对话框接受）
  page.on('dialog', (d) => void d.accept());
  await page.locator('.st-card', { hasText: 'E2E Radio' }).locator('button[aria-label*="删除"], button[aria-label*="Delete"]').click();
  await expect(page.locator('.st-card-name', { hasText: 'E2E Radio' })).toHaveCount(0);
});
