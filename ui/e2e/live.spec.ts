import { expect, test, type Page } from '@playwright/test';

/**
 * 直播流在播放条上的三态（docs/52 §7.1）。
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

test('直播页：地址框打开一个流 → 大画面三态 + 最近打开按地址去重', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '直播' }).click();
  await expect(page.locator('.main h1')).toHaveText('直播');
  // 没开过直播时是空态，不是一行假记录
  await expect(page.locator('.recent-empty')).toBeVisible();

  await page.getByLabel('直播流地址').fill(STREAM_URL);
  await page.getByRole('button', { name: '打开', exact: true }).click();

  // 大画面：标题 + 三态徽章 + 出处
  await expect(page.locator('.live-stage .stage-title')).toHaveText('index');
  await expect(page.locator('.stage-badge')).toHaveText('直播');
  await expect(page.locator('.stage-origin')).toHaveText('stream.example.com');

  // 最近打开：一条，URL 原样留档
  const row = page.locator('.recent-row').first();
  await expect(row.locator('.recent-url')).toHaveText(STREAM_URL);
  await expect(page.locator('.recent-count')).toHaveText('1 / 20');

  // 同一个台再开一次：按地址去重，不新增第二条
  await row.locator('.recent-open').click();
  await expect(page.locator('.recent-count')).toHaveText('1 / 20');

  // 移除后回到空态
  await row.getByRole('button', { name: '从最近打开中移除' }).click();
  await expect(page.locator('.recent-empty')).toBeVisible();
});
