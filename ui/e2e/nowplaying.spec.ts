import { expect, test, type Page } from '@playwright/test';

/**
 * 正在播放页三块可切 + 队列抽屉（B5a 起常驻）。
 *
 * 断言的是**行为契约**，不是版式：
 * - 三块切换：SegmentedControl 一次只渲染一块，切走后上一块卸载；
 * - 队列抽屉：Popover 受控（开 → 内容出现且触发键 aria-expanded=true；
 *   Esc 关闭且焦点回触发键）。跑在浏览器预览的 mock 后端上。
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

async function openNowPlaying(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '正在播放' }).click();
  await expect(page.locator('.main h1')).toHaveText('未在播放');
}

test('正在播放页三块可切：一次只渲染一块，切走即卸载', async ({ page }) => {
  await openNowPlaying(page);

  // 默认歌词块
  await expect(page.locator('.lyrics-panel')).toBeVisible();
  await expect(page.locator('.eq-panel')).toHaveCount(0);

  // 切到均衡器：10 段滑杆 + 开关都在，歌词块卸载
  await page.getByRole('radio', { name: '均衡器' }).click();
  await expect(page.locator('.eq-panel')).toBeVisible();
  await expect(page.locator('.eq-panel .k-slider')).toHaveCount(11); // 10 段 + 前置增益
  await expect(page.locator('.lyrics-panel')).toHaveCount(0);

  // 切到频谱（mock 下频谱默认关 → 显示已关闭占位，同样是"块已切换"的证明）
  await page.getByRole('radio', { name: '频谱' }).click();
  await expect(page.locator('.spectrum-panel')).toBeVisible();
  await expect(page.locator('.eq-panel')).toHaveCount(0);
});

test('均衡器：预设菜单命中打 ✓，应用后滑杆值跟随（B6 补 EQ 交互覆盖）', async ({ page }) => {
  await openNowPlaying(page);
  await page.getByRole('radio', { name: '均衡器' }).click();
  await expect(page.locator('.eq-panel')).toBeVisible();

  // 初始平直：首段值为 0，预设按钮显示「自定义」
  await expect(page.locator('.eq-band').first().locator('.eq-val')).toHaveText('0');

  // 开预设菜单 → 选「重低音」→ 菜单关、按钮显示命中名、滑杆值写入
  await page.locator('.eq-panel header .link').click();
  const menu = page.locator('.k-menu');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: '重低音' }).click();
  await expect(menu).toHaveCount(0);
  await expect(page.locator('.eq-panel header .link')).toHaveText(/预设：重低音/);
  await expect(page.locator('.eq-band').first().locator('.eq-val')).toHaveText('+7');
  // 最后一段（16k）仍是 0 —— 重低音只抬低频
  await expect(page.locator('.eq-band').last().locator('.eq-val')).toHaveText('0');

  // 重置回平直
  await page.getByRole('button', { name: '重置曲线' }).click();
  await expect(page.locator('.eq-band').first().locator('.eq-val')).toHaveText('0');
});

test('队列抽屉：Popover 开合，Esc 关闭后焦点回触发键', async ({ page }) => {
  await page.goto('/');
  // 队列内容与列表无关；进曲库只为拿到稳定的首屏
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main .list .row').first()).toBeVisible();

  const trigger = page.getByRole('button', { name: /播放队列/ });
  await trigger.click();
  const panel = page.locator('.queue-float');
  await expect(panel).toBeVisible();
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');

  // Esc 关闭（bits-ui EscapeLayer），焦点还原到触发键
  await page.keyboard.press('Escape');
  await expect(panel).toHaveCount(0);
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await expect(trigger).toBeFocused();
});
