import { expect, test, type Page } from '@playwright/test';

/**
 * E2E 冒烟（docs/18 §9 大件 C）：UI 装配 + 关键交互链路。
 *
 * 跑在浏览器预览的 mock 后端上（vite preview + 无 Tauri 自动装 mock）。
 * 判据刻意压在「坏了必炸」的锚点上：曲目行渲染、双击播放链路（mock 的
 * playTracks → 状态广播 → 播放条/队列面板联动）、设置分区、频谱 pro 渲染。
 * 另有一条全局哨兵：任何 console.error / pageerror 直接失败。
 */

/** 每个用例的运行时错误收集（beforeEach 清零、afterEach 断言） */
let runtimeErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  runtimeErrors = [];
  page.on('pageerror', (e) => runtimeErrors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') runtimeErrors.push(`console.error: ${m.text()}`);
  });
  // 频谱开关默认关（mock 设置默认值），冒烟里要开频谱面板 → 种好设置
  await page.addInitScript(() => {
    const raw = localStorage.getItem('aiya.settings');
    const s = raw ? JSON.parse(raw) : {};
    s.spectrum = true;
    s.spectrumEngine = 'pro';
    localStorage.setItem('aiya.settings', JSON.stringify(s));
  });
});

test.afterEach(async () => {
  expect(runtimeErrors, '页面运行期错误').toEqual([]);
});

/** 双击第一行触发播放（行有拖拽系统，需要完整指针序列 + dblclick） */
async function playFirstRow(page: Page): Promise<void> {
  const row = page.locator('.main .list .row').first();
  await row.waitFor({ state: 'visible' });
  await row.dispatchEvent('pointerdown', { pointerId: 1, pointerType: 'mouse', isPrimary: true });
  await row.dispatchEvent('pointerup', { pointerId: 1, pointerType: 'mouse', isPrimary: true });
  await row.dblclick();
  // 等播放条出歌名（"未在播放"消失）
  await expect(page.locator('footer')).not.toContainText('未在播放', { timeout: 5_000 });
}

test('启动装配：曲库渲染 + 播放条在位', async ({ page }) => {
  await page.goto('/');
  // 侧边栏与主列表
  await expect(page.locator('.nav-item').first()).toBeVisible();
  await expect(page.locator('.main .list .row').first()).toBeVisible();
  // 233 首 mock 曲库：视图头应显示总数（曲目行是虚拟滚动，别数 DOM）
  await expect(page.locator('.main')).toContainText('233');
  // 播放条空态
  await expect(page.locator('footer')).toContainText('未在播放');
});

test('播放链路：双击播放 → 队列面板激活行', async ({ page }) => {
  await page.goto('/');
  await playFirstRow(page);
  // 播放条有曲目名与时长
  await expect(page.locator('footer')).toContainText(/\d+:\d{2}/);
  // 打开队列面板：当前曲有 ▶ 标记
  await page.locator('footer button[aria-label="播放队列"]').click();
  const active = page.locator('.queue-panel .queue-row.active');
  await expect(active).toHaveCount(1);
  await expect(active.locator('.q-index')).toHaveText('▶');
});

test('设置分区：播放 tab 输出分区在位 + 主题切换生效', async ({ page }) => {
  await page.goto('/');
  await page.locator('.nav-item', { hasText: '设置与诊断' }).click();
  await page.locator('.tabs button', { hasText: '播放' }).click();
  // 大件 A 之后 output 分区是播放 tab 顶部的四行
  await expect(page.locator('.setting-title', { hasText: '输出模式' })).toBeVisible();
  await expect(page.locator('.setting-title', { hasText: '缓冲' })).toBeVisible();
  await expect(page.locator('.setting-title', { hasText: '时钟' })).toBeVisible();
  await expect(page.locator('.setting-title', { hasText: '暂停时释放设备' })).toBeVisible();
  // 主题切换：外观 tab → 浅色 → html 属性变
  await page.locator('.tabs button', { hasText: '外观' }).click();
  await page.locator('.setting-row', { hasText: '主题' }).locator('button', { hasText: '浅色' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
});

test('频谱 pro：面板打开 + 五形态 chip 循环', async ({ page }) => {
  await page.goto('/');
  await playFirstRow(page);
  await page.locator('footer button[aria-label="频谱"]').click();
  const panel = page.locator('.spectrum-panel');
  await expect(panel).toBeVisible();
  // pro 引擎分支的标志：头部有形态 chip（compact 分支没有）
  const chip = panel.locator('.mode-chip');
  await expect(chip).toBeVisible();
  const labels = ['渐变条', 'LED', '镜像', '径向', '反射'];
  for (const label of labels) {
    await expect(chip).toHaveText(label);
    await chip.click();
  }
  // 循环一整圈回到起点
  await expect(chip).toHaveText(labels[0]!);
});
