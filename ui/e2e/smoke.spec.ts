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

/** 曲库页第一行真实数据（排除加载骨架与失败行） */
function firstRow(page: Page) {
  return page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
}

/** 侧栏「曲库」→ 曲库列表（2026-10-28：推荐页仪表盘化，「浏览全部曲目」
 *  按钮撤销，侧栏成为列表页唯一入口） */
async function openLibrary(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await firstRow(page).waitFor({ state: 'visible' });
}

test('启动装配：顶栏四件 + 仪表盘三级 + 播放条空态', async ({ page }) => {
  await page.goto('/');
  // 宽 TitleBar：搜索框在位、返回键此时无历史所以 disabled
  await expect(page.locator('.titlebar .tb-search input')).toBeVisible();
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeDisabled();
  // 推荐页是仪表盘（2026-10-28）：无页标题，歌单行 / 常听榜两级；
  // 三项统计入口二次修订撤销（侧栏已有一级导航），只剩缺失警告位
  await expect(page.locator('.main h1')).toHaveCount(0);
  await expect(page.locator('.main .playlist-row .playlist-card').first()).toBeVisible();
  await expect(page.locator('.main .stat-card')).toHaveCount(0);
  await expect(page.locator('.main .preview-list .row').first()).toBeVisible();
  await expect(page.locator('.main .list')).toHaveCount(0);
  await expect(page.locator('footer')).toContainText('未在播放');
  // 歌单摘要的加载链路：boot → playlists.refresh() → library.playlists → 侧栏。
  // 2026-10-26 这条数据从 ui store 挪到 library（避免 list/scan 与 playlists 成环），
  // 断言在这里是为了「挪完还接得上」——mock 种子固定 4 张。
  await expect(page.locator('.sidebar .playlist-item')).toHaveCount(4);
  // docs/25 §5 T4c/T4d：背景四层是全应用的——推荐页也有背景渐变层；
  // 频谱 mock 下默认关，所以环境层不在。2026-10-28 用户拍板全页面统一
  // 沉浸档：可读性遮罩 veil 整层删除（不再有 .ambient-veil），断言其缺席。
  await expect(page.locator('.ambient-art')).toHaveCount(1);
  await expect(page.locator('.ambient-veil')).toHaveCount(0);
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

  // 再退一步回到推荐页（仪表盘，无页标题），此时栈空 → 返回键 disabled
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveCount(0);
  await expect(page.locator('.main .playlist-row .playlist-card').first()).toBeVisible();
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeDisabled();
});

test('播放链路：双击行播放 → 播放条与队列激活行', async ({ page }) => {
  await openLibrary(page);
  // B4-1 交互语义：单击=选中，双击=播放。双击**标题单元格**——行几何中心
  // 落在歌手/专辑跳转链接上，链接会吃掉第一下单击去导航（2026-10-28 实测）
  await firstRow(page).locator('.cell.title').dblclick();
  // 播放条出歌名与时长（mock 的 playTracks → 状态广播 → store → 渲染）
  await expect(page.locator('footer')).not.toContainText('未在播放', { timeout: 5_000 });
  await expect(page.locator('footer')).toContainText(/\d+:\d{2}/);

  // label 带首数（`播放队列（12 首）`），用 role + 正则匹配前缀，别绑死计数
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  const active = page.locator('.queue-panel .queue-row.active');
  await expect(active).toHaveCount(1);
  await expect(active.locator('.q-index')).toHaveText('▶');
});

test('曲库 tab：最近添加/最近播放切换（2026-10-28 自推荐页迁来）', async ({ page }) => {
  await openLibrary(page);
  const tabs = page.locator('.lib-tabs .lib-tab');
  await expect(tabs).toHaveCount(3);
  await expect(page.locator('.lib-tab.on')).toHaveText('全部曲目');

  // 切「最近添加」：标题与高亮跟随，列表仍在（mock 种子有 added 排序数据）
  await tabs.filter({ hasText: '最近添加' }).click();
  await expect(page.locator('.main h1')).toHaveText('最近添加');
  await expect(page.locator('.lib-tab.on')).toHaveText('最近添加');
  await firstRow(page).waitFor({ state: 'visible' });

  // 同 tab 二次点击 = 翻转正倒序（2026-10-30）：首行必须变化
  const addedDescFirst = await firstRow(page).textContent();
  await tabs.filter({ hasText: '最近添加' }).click();
  await expect(firstRow(page)).not.toHaveText(addedDescFirst ?? '', { timeout: 5_000 });

  // 切「最近播放」：同样支持二次点击翻转（mock 种子 35% 已播，顺序可翻）
  await tabs.filter({ hasText: '最近播放' }).click();
  await expect(page.locator('.main h1')).toHaveText('最近播放');
  await firstRow(page).waitFor({ state: 'visible' });
  const recentDescFirst = await firstRow(page).textContent();
  await tabs.filter({ hasText: '最近播放' }).click();
  await expect(firstRow(page)).not.toHaveText(recentDescFirst ?? '', { timeout: 5_000 });

  // 回「全部曲目」：恢复原 tab 语义
  await tabs.filter({ hasText: '全部曲目' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await firstRow(page).waitFor({ state: 'visible' });

  // 歌单页没有 tab（tab 是曲库页的子状态）
  await page.locator('.sidebar .playlist-item').first().click();
  await expect(page.locator('.lib-tabs')).toHaveCount(0);
});

test('设置页：单页平铺 + tab 跳转定位 + 主题切换生效', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  // 2026-10-28 三次修订：改回单页平铺——大标题固定「设置」，tab 只做跳转
  await expect(page.locator('.main h1')).toHaveText('设置');
  await expect(page.locator('.main .ch-kicker')).toHaveCount(0);

  // 五个 tab，初始高亮第一组；五个区块全部渲染（含小标题）。
  // 语义是"滚动到某个区块"而不是"切换面板"→ nav + aria-current（同 content/Toolbar）
  const tabs = page.locator('.settings-tabs .settings-tab');
  await expect(tabs).toHaveCount(5);
  await expect(tabs.first()).toHaveAttribute('aria-current', 'true');
  await expect(page.locator('.group-title')).toHaveCount(5);
  await expect(page.locator('.group-title').first()).toHaveText('外观');

  // 点「快捷键」→ 平滑滚动跳转，内容本就全渲染（无条件切换）
  const shortcutTab = tabs.filter({ hasText: '快捷键' });
  await shortcutTab.click();
  await expect(page.locator('.shortcut-list')).toBeVisible();
  await expect(shortcutTab).toHaveAttribute('aria-current', 'true');
  await expect(tabs.first()).not.toHaveAttribute('aria-current', 'true');
  await expect(page.locator('.main h1')).toHaveText('设置');

  // 主题切换即时生效（写到 <html data-theme>）；外观区块平铺常在，直接点
  await page
    .locator('.setting-row', { hasText: '主题' })
    .locator('button', { hasText: '浅色' })
    .click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
});

test('正在播放页：起播后两栏沉浸布局 + 背景四层接住真实曲目', async ({ page }) => {
  await openLibrary(page);
  await firstRow(page).locator('.cell.title').dblclick();
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
  // 2026-10-30 修：这条断言原先盯 `.ambient-spectrum`，而 docs/46 之后那个
  // canvas 只在 **WebGL 不可用的 2D 兜底**路径上才存在，正常机器上恒为空——
  // 断言"为 0"是真的，只是它什么都没验。正在播放页的频谱现在是页内
  // `<canvas class="np-spectrum">`，由「正在播放页频谱」开关派生。
  await expect(page.locator('.np-spectrum')).toHaveCount(0); // mock 默认关

  // 侧栏在这一页整体退出（docs/46 §5.2），所以要去设置得先退出去；
  // Esc 就是那一页的退路（keyboard.ts 里显式给的语义）。
  // 2026-10-30 修：这条用例以前直接点侧栏「设置」，从 docs/46 之后稳定超时。
  await page.keyboard.press('Escape');
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  // 开关名也是 docs/46 改的：原来一个「频谱可视化」，现在拆成背景/正在播放页两个
  await page.getByRole('switch', { name: '正在播放页频谱' }).click();
  await page.locator('footer .now').click();
  await expect(page.locator('.np-spectrum')).toHaveCount(1);
});
