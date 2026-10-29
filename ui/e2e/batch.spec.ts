import { expect, test, type Page } from '@playwright/test';

/**
 * 列表批量操作回归（docs/27 重写）：
 * 悬浮批量条已删，批量动作内置在 TrackList 顶部工具条（ListToolbar）。
 * 覆盖：Ctrl+点行自动进批量模式、工具条加入队列、批量开关与勾选列显隐、
 * 首页预览表为纯列表（无勾选）。
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

function firstRow(page: Page) {
  return page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
}

async function openLibrary(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await firstRow(page).waitFor({ state: 'visible' });
}

test('列表页：Ctrl+点两行自动进批量模式 → 工具条加入队列', async ({ page }) => {
  await openLibrary(page);

  // 批量模式默认关：勾选框不可见，单选行仍然高亮
  // （点标题单元格：行中心的歌手/专辑是可点跳转链接，行点击守卫会忽略）
  const toolbar = page.locator('.list-toolbar');
  await expect(toolbar).toBeVisible();
  const rowA = firstRow(page);
  const rowB = page.locator('.main .list .row:not(.skeleton):not(.failed)').nth(1);
  await rowA.locator('.cell.title').click();
  await expect(rowA).toHaveClass(/selected/);
  await expect(page.locator('.main .list .k-check')).toHaveCount(0);

  // Ctrl+点第二行 → 批量模式自动打开（勾选列出现），两行都在选择集里
  await rowB.locator('.cell.title').click({ modifiers: ['Control'] });
  await expect(page.locator('.main .list .k-check').first()).toBeVisible();
  await expect(rowA).toHaveClass(/selected/);
  await expect(rowB).toHaveClass(/selected/);
  await expect(toolbar).toContainText(/已选 2 首/);

  // 工具条加入队列：toast + 选择清空（计数消失）
  await toolbar.getByRole('button', { name: '加入播放队列' }).click();
  await expect(page.locator('body')).toContainText(/已加入队列 2 首/);
  await expect(toolbar).not.toContainText(/已选/);
  await expect(rowA).not.toHaveClass(/selected/);
});

test('列表页：批量开关控制勾选列显隐；筛选器打开模态框', async ({ page }) => {
  await openLibrary(page);
  const toolbar = page.locator('.list-toolbar');

  // docs/28：批量开关收成纯图标按钮（aria-pressed 表达开关态）
  const batchToggle = toolbar.getByRole('button', { name: '批量操作' });
  await expect(batchToggle).toHaveAttribute('aria-pressed', 'false');
  await batchToggle.click();
  await expect(batchToggle).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.main .list .k-check').first()).toBeVisible();

  // 筛选器模态框（funnel 图标按钮）：应用「从未播放」→ 列表仍在
  await toolbar.getByRole('button', { name: /筛选器/ }).click();
  const dialog = page.locator('.k-dlg');
  await expect(dialog).toContainText('筛选曲目');
  // mock 种子里有从未播放的曲目，应用后列表仍在
  await dialog.getByRole('button', { name: '应用' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await firstRow(page).waitFor({ state: 'visible' });
});

test('首页：预览列表为纯列表（无勾选列），双击播放保留', async ({ page }) => {
  await page.goto('/');
  // 2026-10-28：预览列表与主列表同外观（复用 .row 网格），包在 .preview-list 里
  const row = page.locator('.main .preview-list .row').first();
  await row.waitFor({ state: 'visible' });

  // 纯预览：没有勾选框、没有悬浮批量条
  await expect(page.locator('.main .preview-list .k-check')).toHaveCount(0);
  await expect(page.locator('.batch-bar')).toHaveCount(0);

  // 双击播放链路保留
  await row.dblclick();
  await expect(page.locator('footer')).not.toContainText('未在播放', { timeout: 5_000 });
});
