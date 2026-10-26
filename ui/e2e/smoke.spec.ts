import { expect, test, type Page } from '@playwright/test';

/**
 * E2E 冒烟（2026-10-26 按新壳体重写）。
 *
 * **选择器原则：认语义与稳定类名，不认版式类名。**
 * 上一版 4 条全红，就是钉在了重构前的版式上：
 * - `.main .list .row` → 首页改表格后那段是 `.tt-row`（`.list` 只在曲库/歌单页）
 * - `'设置与诊断'` → 现在就叫「设置」
 * - `.tabs button` → 设置已改成一页常驻区块 + PageToolbar 锚点，没有 tab 切换
 *
 * 这一版使用：
 * - 壳体：`.titlebar`（唯一全局条）、`.sidebar .nav-item`（一级导航）
 * - 页内：`.main h1`（Header 标题，故意不带类名 —— 标题类名会随批次变）、`.content-toolbar .pt-tab`（Toolbar）
 * - 列表：`.list .row`（虚拟列表）、`.tt-row`（首页紧凑表）
 * 版式细节（`.page-pad`、`.tt-*`、间距）**不参与断言** —— 那些会继续变。
 *
 * 跑在浏览器预览的 mock 后端上（无 Tauri 时 main.ts 自动装 mock），
 * 真引擎行为归 Rust 单测与真窗口探针（tools/cdp-probe.mjs）。
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

/** 列表里第一行真实数据（排除加载骨架与失败行） */
function firstRow(page: Page) {
  return page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
}

/** 首页 → PageHeader 的「浏览全部曲目」→ 曲库列表 */
async function openLibrary(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.main button', { hasText: '浏览全部曲目' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await firstRow(page).waitFor({ state: 'visible' });
}

test('启动装配：顶栏四件 + 推荐页标题 + 播放条空态', async ({ page }) => {
  await page.goto('/');
  // 宽 TitleBar：搜索框在位、返回键此时无历史所以 disabled
  await expect(page.locator('.titlebar .tb-search input')).toBeVisible();
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeDisabled();
  // 页内标题归 PageHeader（不是顶栏）。docs/25 §3：「首页」改名「推荐」
  await expect(page.locator('.main h1')).toHaveText('推荐');
  await expect(page.locator('.main .ch-kicker')).toHaveText('YOUR LIBRARY');
  // 首页三段都是紧凑表（TrackTable），不是虚拟列表
  await expect(page.locator('.main .tt-row').first()).toBeVisible();
  await expect(page.locator('.main .list')).toHaveCount(0);
  await expect(page.locator('footer')).toContainText('未在播放');
  // 歌单摘要的加载链路：boot → playlists.refresh() → library.playlists → 侧栏。
  // 2026-10-26 这条数据从 ui store 挪到 library（避免 list/scan 与 playlists 成环），
  // 断言在这里是为了「挪完还接得上」——mock 种子固定 4 张。
  await expect(page.locator('.sidebar .playlist-item')).toHaveCount(4);
  // docs/25 §5 T4c/T4d：背景四层是全应用的——推荐页也有背景渐变层与遮罩层；
  // 频谱 mock 下默认关，所以环境层不在
  await expect(page.locator('.ambient-art')).toHaveCount(1);
  await expect(page.locator('.ambient-veil')).toBeVisible();
  await expect(page.locator('.ambient-spectrum')).toHaveCount(0);
});

test('歌单：侧栏点开 → 标题与列表来自该歌单', async ({ page }) => {
  await page.goto('/');
  const first = page.locator('.sidebar .playlist-item').first();
  const name = (await first.getAttribute('title'))?.split(' · ')[0] ?? '';
  await first.click();
  await expect(page.locator('.main h1')).toHaveText(name);
  await expect(firstRow(page)).toBeVisible();
  // 曲库三 tab 在歌单视图里不出现（工具条全空时整条不渲染）
  await expect(page.locator('.main .content-toolbar')).toHaveCount(0);
});

test('导航链：曲库 → 侧栏切专辑宫格（docs/28 一级项）→ 详情 → 逐级返回', async ({ page }) => {
  await openLibrary(page);
  // docs/28：曲库页第一层 tab 已撤（连同分隔线），专辑/艺人从侧栏进
  await expect(page.locator('.main .content-toolbar .pt-tab')).toHaveCount(0);

  await page.locator('.sidebar .nav-item', { hasText: '专辑' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');

  // 进详情页再逐级返回：详情 → 宫格 → 全部曲目 → 推荐页（栈空，返回键 disabled）
  await page.locator('.main .k-card').first().click();
  await expect(page.locator('.main .list .row').first()).toBeVisible();
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeEnabled();

  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeEnabled();

  // 再退一步回到推荐页（原首页），此时栈空 → 返回键 disabled
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('推荐');
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeDisabled();
});

test('播放链路：双击行播放 → 播放条与队列激活行', async ({ page }) => {
  await openLibrary(page);
  // B4-1 交互语义：单击=选中，双击=播放
  await firstRow(page).dblclick();
  // 播放条出歌名与时长（mock 的 playTracks → 状态广播 → store → 渲染）
  await expect(page.locator('footer')).not.toContainText('未在播放', { timeout: 5_000 });
  await expect(page.locator('footer')).toContainText(/\d+:\d{2}/);

  // label 带首数（`播放队列（12 首）`），用 role + 正则匹配前缀，别绑死计数
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  const active = page.locator('.queue-panel .queue-row.active');
  await expect(active).toHaveCount(1);
  await expect(active.locator('.q-index')).toHaveText('▶');
});

test('设置页：左分组导航 + scroll-spy + 主题切换生效', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await expect(page.locator('.main h1')).toHaveText('设置');
  await expect(page.locator('.main .ch-kicker')).toHaveText('PREFERENCES');

  // docs/29 批 A：左栏收窄为四组（音乐库与任务/诊断 → 扫描入库页）；初始高亮第一组
  const rail = page.locator('.settings-rail .rail-item');
  await expect(rail).toHaveCount(4);
  await expect(rail.first()).toHaveAttribute('aria-current', 'true');

  // 点「快捷键」→ 平滑滚动后 scroll-spy 把高亮写回快捷键组
  await rail.last().click();
  await expect(page.locator('#settings-shortcuts')).toBeInViewport();
  await expect(rail.last()).toHaveAttribute('aria-current', 'true');
  await expect(rail.first()).not.toHaveAttribute('aria-current', 'true');

  // 主题切换即时生效（写到 <html data-theme>）
  await page
    .locator('.setting-row', { hasText: '主题' })
    .locator('button', { hasText: '浅色' })
    .click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
});

test('正在播放页：起播后两栏沉浸布局 + 背景四层接住真实曲目', async ({ page }) => {
  await openLibrary(page);
  await firstRow(page).dblclick();
  await expect(page.locator('footer')).not.toContainText('未在播放', { timeout: 5_000 });

  // docs/25 §6：正在播放页唯一入口 = 播放条左侧歌曲区域（侧栏席位已撤）
  await page.locator('footer .now').click();

  // docs/25 §4 两栏改版：信息列跟着播放态走（标题换成真歌名），歌词容器在右
  await expect(page.locator('.np-info h1')).not.toHaveText('未在播放');
  await expect(page.locator('.np-info .audio-spec')).toBeVisible();
  await expect(page.locator('.lyrics-panel')).toBeVisible();

  // docs/25 §5 背景四层：背景层常在；沉浸档（正在播放页）遮罩撤销（T4d）——
  // 频谱环境光不被压暗
  await expect(page.locator('.ambient-art')).toBeVisible();
  await expect(page.locator('.ambient-veil')).toHaveCount(0);
  await expect(page.locator('.ambient-spectrum')).toHaveCount(0); // mock 下频谱默认关

  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await page.getByRole('switch', { name: '频谱可视化' }).click();
  await page.locator('footer .now').click();
  await expect(page.locator('.ambient-spectrum')).toHaveCount(1);
});
