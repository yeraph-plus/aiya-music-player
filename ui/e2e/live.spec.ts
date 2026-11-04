import { expect, test, type Page } from '@playwright/test';

/**
 * 直播流在播放条上的三态。
 *
 * 起点用 `?live=<url>` 把 mock 直接摆到"正在播一个直播流"（main.ts 里那个
 * query 参数的来由写在 `installMock` 的注释里）。断言的是**契约字段驱动的
 * 渲染分支**：
 * - `source === 'stream'` → 进度滑块换成呼吸条（不可拖）；
 * - `durationMs === null` → 时长位不再渲染 `0:00`，换成三态徽章；
 * - 暂停 = 断开（位置归零、徽章变「已断开」），再播放 = 重连。
 */

let runtimeErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  runtimeErrors = [];
  page.on('pageerror', (e) => runtimeErrors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') runtimeErrors.push(`console.error: ${m.text()}`);
  });
});

test.afterEach(async () => {
  expect(runtimeErrors, '页面运行期错误').toEqual([]);
});

const STREAM_URL = 'http://stream.example.com/hls/index.m3u8';

async function openStream(page: Page): Promise<void> {
  await page.goto(`/?live=${encodeURIComponent(STREAM_URL)}`);
  await expect(page.locator('footer.player')).toBeVisible();
}

test('直播：进度条换成呼吸条，时长位是「直播」而不是 0:00', async ({ page }) => {
  await openStream(page);

  // 呼吸条在，进度滑块不在（直播没有可拖的刻度）
  await expect(page.locator('footer .timeline .live-bar')).toBeVisible();
  await expect(page.getByRole('slider', { name: '播放进度' })).toHaveCount(0);

  // 徽章是「直播」，不是 `formatTime(null)` 那个 0:00
  const badge = page.locator('footer .timeline .live-badge');
  await expect(badge).toHaveText('直播');
  await expect(badge).toHaveClass(/on/);
  await expect(page.locator('footer .timeline > .t').last()).not.toHaveText('0:00');

  // 副标题是主机名（临时项没有艺人，不能落成「未知艺人」）
  await expect(page.locator('footer .line2 .artist')).toHaveText('stream.example.com');
  await expect(page.locator('footer .meta .title')).toHaveText('index');

  // 直播项是临时项：不能加入播放列表（写进去就是一行查不到的坏数据）
  await expect(page.getByRole('button', { name: '临时项不能加入播放列表' })).toBeDisabled();
});

test('直播：位置按秒推进（已收听时长），暂停即断开并归零', async ({ page }) => {
  await openStream(page);

  const listened = page.locator('footer .timeline > .t').first();
  await expect(listened).toHaveText(/^0:0[1-9]$/, { timeout: 5000 });

  // 暂停 = 断开连接（不是"保持连接挂起"）
  await page.getByRole('button', { name: '断开直播' }).click();
  const badge = page.locator('footer .timeline .live-badge');
  await expect(badge).toHaveText('已断开');
  await expect(badge).not.toHaveClass(/on/);
  // 断开后位置归零：连接没了，已收听时长自然也不该停在原地
  await expect(listened).toHaveText('0:00');
  // 呼吸条停下来（不再显示"还在播"的假象）
  await expect(page.locator('footer .timeline .live-bar')).not.toHaveClass(/on/);

  // 播放 = 重连
  await page.getByRole('button', { name: '连接直播' }).click();
  await expect(badge).toHaveText('直播');
  await expect(page.locator('footer .timeline .live-bar')).toHaveClass(/on/);
  await expect(listened).toHaveText(/^0:0[1-9]$/, { timeout: 5000 });
});

test('队列面板：临时项自带标题与来源标记，不等库内元数据', async ({ page }) => {
  await openStream(page);

  await page.getByRole('button', { name: /^播放队列/ }).click();
  const row = page.locator('.queue-list .queue-row').first();
  await expect(row).toBeVisible();

  // 标题来自快照（`QueueSnapshot.transient`），不是"载入中…"占位
  await expect(row.locator('.q-title')).toHaveText('index');
  await expect(row).not.toContainText('载入中');
  // 来源标记 + 时长位都按直播口径
  await expect(row.locator('.q-tag')).toHaveText('直播');
  await expect(row.locator('.q-duration')).toHaveText('直播');
});

test('直播页：模态添加流 → 卡组按地址去重 → 点中央卡开播 → 移除后只剩加号卡', async ({ page }) => {
  // 预览 mock 对全新会话播种三条演示流；本用例验的是**空卡组机制**，
  // 先写一份显式空设置顶掉种子（保存过的状态 mock 一律尊重）
  await page.addInitScript(() => {
    localStorage.setItem('aiya.settings', JSON.stringify({ liveRecent: [] }));
  });
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '直播' }).click();
  await expect(page.locator('.main h1')).toHaveText('直播');
  // 卡组只有流 + 队尾常驻加号卡：没有流时加号卡就是焦点（居中）
  const addCard = page.locator('.live-card.add-card');
  await expect(addCard).toBeVisible();
  await expect(addCard).toHaveClass(/pos-current/);
  await expect(page.locator('.live-card:not(.add-card)')).toHaveCount(0);

  // 焦点加号卡 → 点它弹出新增模态框
  await addCard.click();
  await page.getByLabel('m3u8 地址').fill(STREAM_URL);
  await page.getByLabel('名称（可选）').fill('测试流');
  await page.getByRole('button', { name: '添加', exact: true }).click();

  // 卡组：一张流卡（新流在头部即焦点）+ 队尾加号卡
  const card = page.locator('.live-card:not(.add-card)');
  await expect(card).toHaveCount(1);
  await expect(card).toHaveClass(/pos-current/);

  // 同一个地址再添加一次：按地址去重，不新增第二张流卡
  await page.locator('.content-header').getByRole('button', { name: '添加流', exact: true }).click();
  await page.getByLabel('m3u8 地址').fill(STREAM_URL);
  await page.getByRole('button', { name: '添加', exact: true }).click();
  await expect(page.locator('.live-card:not(.add-card)')).toHaveCount(1);

  // 点焦点卡开播：信息块徽章与播放条徽章同时进入直播态
  await page.locator('.live-card.pos-current .card-face').click();
  await expect(page.locator('.live-info .stage-badge')).toHaveText('直播');
  await expect(page.locator('footer .timeline .live-badge')).toHaveText('直播');

  // 移除后卡组只剩队尾加号卡，且它成为焦点
  await page.locator('.live-card.pos-current .card-remove').click();
  await expect(page.locator('.live-card:not(.add-card)')).toHaveCount(0);
  await expect(page.locator('.live-card.add-card')).toHaveClass(/pos-current/);
});
