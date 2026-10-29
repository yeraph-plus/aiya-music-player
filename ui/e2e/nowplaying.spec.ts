import { expect, test, type Page } from '@playwright/test';

/**
 * 正在播放页两栏沉浸 + EQ/队列抽屉（docs/25 §6 起 SideDrawer 宿主）。
 *
 * 断言的是**行为契约**，不是版式：
 * - 正在播放入口：播放条歌曲区域（侧栏无席位）；
 * - EQ 抽屉：从播放条进入（SideDrawer 受控），预设/滑杆交互同 B6；
 * - 队列抽屉：开 → 内容出现且触发键 aria-expanded=true；Esc 关闭且焦点回
 *   触发键；EQ 与队列互斥。跑在浏览器预览的 mock 后端上。
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
  // docs/25 §6：正在播放页唯一入口是播放条左侧歌曲区域（侧栏席位已撤）
  await page.locator('footer .now').click();
  await expect(page.locator('.main h1')).toHaveText('未在播放');
}

test('正在播放页两栏沉浸：左信息右歌词，卡片与切换条已拆', async ({ page }) => {
  await openNowPlaying(page);

  // 左信息列 + 右歌词都在；EQ 在播放条抽屉（docs/25 §1），不在页面里
  await expect(page.locator('.np-info')).toBeVisible();
  await expect(page.locator('.lyrics-panel')).toBeVisible();
  await expect(page.locator('.eq-panel')).toHaveCount(0);

  // 去卡片化（docs/25 §4）：hero 卡片与 SegmentedControl 不复存在
  await expect(page.locator('.now-playing-hero')).toHaveCount(0);
  await expect(page.locator('.now-playing-tabs')).toHaveCount(0);
});

test('EQ 抽屉：从播放条进入，预设命中打 ✓；与队列抽屉互斥', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main .list .row').first()).toBeVisible();

  const eqTrigger = page.getByRole('button', { name: /^均衡器/ });
  await eqTrigger.click();
  const panel = page.locator('.eq-panel');
  await expect(panel).toBeVisible();
  // 2026-10-28 三区制：只剩 10 段滑杆；低音/高音/前置增益三旋钮（三角布局）
  await expect(panel.locator('.k-slider')).toHaveCount(10);
  await expect(panel.locator('.knob[role="slider"]')).toHaveCount(3);

  // 开预设菜单 → 选「重低音」→ 菜单关、按钮显示命中名、滑杆值写入
  await panel.locator('.preset-btn').click();
  const menu = page.locator('.k-menu');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: '重低音' }).click();
  await expect(menu).toHaveCount(0);
  await expect(panel.locator('.preset-btn')).toHaveText(/重低音/);
  await expect(page.locator('.eq-band').first().locator('.eq-val')).toHaveText('+7');
  // 最后一段（16k）仍是 0 —— 重低音只抬低频
  await expect(page.locator('.eq-band').last().locator('.eq-val')).toHaveText('0');
  // 启用后触发键打点语义：aria-label 更新
  await expect(eqTrigger).toHaveAttribute('aria-label', '均衡器（已启用）');

  // 互斥：开队列 → EQ 抽屉被关掉（ui store 单向清理，docs/25 §1）
  const queueTrigger = page.locator('footer').getByRole('button', { name: /播放队列/ });
  await queueTrigger.click();
  await expect(page.locator('.queue-float')).toBeVisible();
  await expect(page.locator('.eq-panel')).toHaveCount(0);

  // 反向互斥：再开 EQ → 队列被关掉
  await eqTrigger.click();
  await expect(page.locator('.eq-panel')).toBeVisible();
  await expect(page.locator('.queue-float')).toHaveCount(0);

  // 重置回平直（顺便验证抽屉内交互仍全通）
  await page.getByRole('button', { name: '重置曲线' }).click();
  await expect(page.locator('.eq-band').first().locator('.eq-val')).toHaveText('0');
});

test('队列抽屉：Popover 开合，Esc 关闭后焦点回触发键', async ({ page }) => {
  await page.goto('/');
  // 队列内容与列表无关；进曲库只为拿到稳定的首屏
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main .list .row').first()).toBeVisible();

  const trigger = page.locator('footer').getByRole('button', { name: /播放队列/ });
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
