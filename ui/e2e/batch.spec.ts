import { expect, test, type Page } from '@playwright/test';

/**
 * 批量条回归（B4-2b 起常驻）：全应用唯一渲染点在 AppShell，
 * 首页三段（ID 选择集）与列表页（下标选择集）都汇到 `stores/batch.svelte.ts`。
 * 覆盖：出现/消失、加入队列、跨段互斥、✕ 清空。
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

test('首页：选中一段 → 批量条出现 → 加入队列 → 批量条消失', async ({ page }) => {
  await page.goto('/');
  const row = page.locator('.main .tt-row').first();
  await row.click();
  await expect(row).toHaveClass(/selected/);

  // 批量条（AppShell 渲染的全应用唯一实例）
  const bar = page.locator('.batch-bar');
  await expect(bar).toBeVisible();
  await expect(bar).toContainText(/已选 1 首/);

  // 加入队列：toast + 批量条消失（选择被清）
  await bar.getByRole('button', { name: /加入队列/ }).click();
  await expect(page.locator('body')).toContainText(/已加入队列 1 首/);
  await expect(bar).toHaveCount(0);

  // 换段互斥：在 A 段选中后点 B 段，A 段清空
  const rowA = page.locator('.home-section', { hasText: '最近播放' }).locator('.tt-row').first();
  const rowB = page.locator('.home-section', { hasText: '常听' }).locator('.tt-row').first();
  await rowA.click();
  await expect(rowA).toHaveClass(/selected/);
  await expect(bar).toBeVisible();
  await rowB.click();
  await expect(rowB).toHaveClass(/selected/);
  await expect(rowA).not.toHaveClass(/selected/);
});

test('列表页：选中 → 批量条 → ✕ 清空', async ({ page }) => {
  await page.goto('/');
  await page.locator('.main button', { hasText: '浏览全部曲目' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  const row = page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
  await row.waitFor({ state: 'visible' });

  await row.click();
  const bar = page.locator('.batch-bar');
  await expect(bar).toBeVisible();

  await bar.getByRole('button', { name: '取消选择' }).click();
  await expect(bar).toHaveCount(0);
  await expect(row).not.toHaveClass(/selected/);
});
