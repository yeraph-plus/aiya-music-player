import { expect, test, type Page } from '@playwright/test';

/**
 * 路由与 UiState 迁移回归（B4-1 起常驻，原为临时探针）。
 *
 * 两块最容易**静默**坏的东西：
 * 1. 旧 UiState 文件的迁移 —— 写错了不报错，只表现成"重启后回到首页、
 *    用户以为丢了位置"，所以必须拿真实 localStorage 种子回放。
 * 2. 八值路由各自可达 + 侧栏高亮**恰好一个**（多个高亮=判断写重了，
 *    零个=路由落了但导航没跟上）。
 */

const UI_STATE_KEY = 'aiya.uiState';

async function seedState(page: Page, state: unknown): Promise<void> {
  await page.addInitScript(
    ({ key, value }) => {
      window.localStorage.setItem(key, JSON.stringify(value));
    },
    { key: UI_STATE_KEY, value: state },
  );
}

async function activeSidebar(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll('.sidebar .nav-item, .sidebar .playlist-item')]
      .filter((el) => el.classList.contains('active'))
      .map((el) => el.textContent?.trim() ?? ''),
  );
}

test('旧 UiState：home + libraryDetail + playlist:pl-1 → 落到该歌单，下次落盘换新形状', async ({
  page,
}) => {
  await seedState(page, {
    page: 'home',
    libraryDetail: true,
    view: 'playlist:pl-1',
    sort: 'title',
    direction: 'asc',
    scrollTop: 0,
  });
  await page.goto('/');
  await expect(page.locator('.main h1')).toContainText('华语偏爱');
  await expect(page.locator('.sidebar .playlist-item.active')).toHaveCount(1);

  // restore() 只在**内存**里迁移，不主动落盘（防抖保存跟用户操作走）——
  // 旧字段要等下一次操作才被新形状覆盖。导航一次，验证落盘的已是新形状。
  await page.locator('.sidebar .nav-item', { hasText: '常听' }).click();
  await expect(page.locator('.main h1')).toHaveText('常听');
  await page.waitForTimeout(900); // 防抖
  const stored = await page.evaluate((key) => localStorage.getItem(key), UI_STATE_KEY);
  expect(stored).not.toBeNull();
  const parsed = JSON.parse(stored!) as Record<string, unknown>;
  expect(parsed).toMatchObject({ page: 'mostPlayed', playlistId: null });
  expect(parsed).not.toHaveProperty('libraryDetail');
  expect(parsed).not.toHaveProperty('view');
});

test('旧 UiState：home + libraryDetail + recent → 落到最近播放', async ({ page }) => {
  await seedState(page, { page: 'home', libraryDetail: true, view: 'recent' });
  await page.goto('/');
  await expect(page.locator('.main h1')).toHaveText('最近播放');
  expect(await activeSidebar(page)).toEqual(['最近播放']);
});

test('旧 UiState：home 且无 libraryDetail → 推荐页', async ({ page }) => {
  await seedState(page, { page: 'home', libraryDetail: false, view: 'library' });
  await page.goto('/');
  await expect(page.locator('.main h1')).toHaveText('推荐');
  expect(await activeSidebar(page)).toEqual(['推荐']);
});

test('新形状往返：切到常听 → 落盘 page=mostPlayed → 刷新仍在常听', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '常听' }).click();
  await expect(page.locator('.main h1')).toHaveText('常听');
  await page.waitForTimeout(900); // 防抖
  const stored = await page.evaluate((key) => localStorage.getItem(key), UI_STATE_KEY);
  expect(JSON.parse(stored!)).toMatchObject({ page: 'mostPlayed', playlistId: null });
  await page.reload();
  await expect(page.locator('.main h1')).toHaveText('常听');
});

test('八值路由逐个可达 + 侧栏高亮恰好一个', async ({ page }) => {
  await page.goto('/');
  const cases: Array<{ via: 'initial' | 'nav' | 'playlist'; text: string; title: string }> = [
    { via: 'initial', text: '', title: '推荐' },
    { via: 'nav', text: '曲库', title: '全部曲目' },
    { via: 'nav', text: '最近播放', title: '最近播放' },
    { via: 'nav', text: '常听', title: '常听' },
    // docs/25 §3：FM 电台回归一级导航（占位页）
    { via: 'nav', text: 'FM 电台', title: 'FM 电台' },
    { via: 'playlist', text: '', title: '' }, // 标题动态取歌单名
    { via: 'nav', text: '正在播放', title: '未在播放' },
    { via: 'nav', text: '设置', title: '设置' },
  ];

  for (const [i, c] of cases.entries()) {
    if (c.via === 'nav') {
      await page.locator('.sidebar .nav-item', { hasText: c.text }).click();
    } else if (c.via === 'playlist') {
      await page.locator('.sidebar .playlist-item').first().click();
    }
    await expect(page.locator('.main h1').first()).toHaveText(
      c.via === 'playlist'
        ? (await page.locator('.sidebar .playlist-item').first().getAttribute('title'))!.split(' · ')[0]!
        : c.title,
    );
    // 每一步侧栏都恰好一个高亮
    await expect(page.locator('.sidebar .active')).toHaveCount(1);
    // radio 路由之后落到非列表页，再回列表要能恢复（防 playlistId 残留）
    if (i === cases.length - 1) {
      await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
      await expect(page.locator('.main h1')).toHaveText('全部曲目');
    }
  }
});

test('B4-3b：专辑宫格 → 卡片 → 专辑详情 → 顶栏返回回宫格', async ({ page }) => {
  await page.goto('/');
  // docs/25 §3：专辑宫格从曲库页 tab 进（一级导航已撤）
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await page.locator('.main .content-toolbar .pt-tab', { hasText: '专辑' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  // 侧栏高亮落在「曲库」——专辑已并入曲库 tab，不再是独立一级项
  expect(await activeSidebar(page)).toEqual(['曲库']);

  const card = page.locator('.main .k-card').first();
  const albumTitle = await card.locator('.k-card-title').textContent();
  await card.click();

  // 详情页：标题 = 专辑名，meta 带曲目数（B4-3a 聚合的字段）
  await expect(page.locator('.main h1')).toHaveText(albumTitle!.trim());
  await expect(page.locator('.content-header, .main').first()).toContainText(/首/);
  // 曲目列表是同一查询路径（albumId 筛选），行数 > 0
  await expect(page.locator('.main .list .row').first()).toBeVisible();

  // 返回：回宫格，tab 高亮回「专辑」、侧栏高亮「曲库」
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  expect(await activeSidebar(page)).toEqual(['曲库']);
});

test('B4-3b：艺人详情页列出该艺人的曲目', async ({ page }) => {
  await page.goto('/');
  // docs/25 §3：艺人宫格从曲库页 tab 进（一级导航已撤）
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await page.locator('.main .content-toolbar .pt-tab', { hasText: '艺人' }).click();
  await expect(page.locator('.main h1')).toHaveText('艺人');

  const card = page.locator('.main .k-card').first();
  const artistName = (await card.locator('.k-card-title').textContent())!.trim();
  await card.click();

  await expect(page.locator('.main h1')).toHaveText(artistName);
  // 曲目列表（查询走 artistId 筛选）首行内容含这位艺人——不猜列下标，
  // 列顺序属于 TrackList 的实现细节
  await expect(page.locator('.main .list .row').first()).toContainText(artistName);
});

test('B4-3c：搜索落独立结果页，三段（曲目/专辑/艺人）+ 卡片进详情 + 返回词不丢', async ({
  page,
}) => {
  await page.goto('/');
  // mock 数据：「深」同时命中曲目（艺人名）、专辑（深的深）、艺人（周深）
  const box = page.getByRole('searchbox');
  await box.fill('深');
  await page.waitForTimeout(500); // 曲目 120ms + 卡片段 200ms 两层防抖

  await expect(page.locator('.main h1')).toHaveText('搜索「深」');
  // 按 h2 文本锁定段（卡片副标题也含"专辑/艺人"字样，hasText 会串段）
  const albumRow = page.locator('.facet-row').filter({
    has: page.locator('h2', { hasText: /^专辑$/ }),
  });
  const artistRow = page.locator('.facet-row').filter({
    has: page.locator('h2', { hasText: /^艺人$/ }),
  });
  await expect(albumRow).toBeVisible();
  await expect(artistRow).toBeVisible();
  await expect(page.locator('.main .list .row').first()).toBeVisible();

  // 点专辑卡 → 专辑详情 → 返回：搜索页与搜索词都还在
  await albumRow.locator('.k-card').first().click();
  await expect(page.locator('.main h1')).not.toHaveText('搜索「深」');
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('搜索「深」');
  await expect(box).toHaveValue('深');
});
