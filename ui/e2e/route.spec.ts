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
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  await page.waitForTimeout(900); // 防抖
  const stored = await page.evaluate((key) => localStorage.getItem(key), UI_STATE_KEY);
  expect(stored).not.toBeNull();
  const parsed = JSON.parse(stored!) as Record<string, unknown>;
  expect(parsed).toMatchObject({ page: 'library', playlistId: null });
  expect(parsed).not.toHaveProperty('libraryDetail');
  expect(parsed).not.toHaveProperty('view');
});

test('旧 UiState：home + libraryDetail + recent → 单页已退役，落到曲库（docs/28）', async ({
  page,
}) => {
  await seedState(page, { page: 'home', libraryDetail: true, view: 'recent' });
  await page.goto('/');
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  expect(await activeSidebar(page)).toEqual(['曲库']);
});

// 旧文件 page 直接是退役值的形状（B4-1 拆页后写过一段时间）也要兜底
test('旧 UiState：page=mostPlayed → 落到曲库（docs/28）', async ({ page }) => {
  await seedState(page, { page: 'mostPlayed', sort: 'title', direction: 'asc', scrollTop: 0 });
  await page.goto('/');
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  expect(await activeSidebar(page)).toEqual(['曲库']);
});

test('旧 UiState：home 且无 libraryDetail → 推荐页', async ({ page }) => {
  await seedState(page, { page: 'home', libraryDetail: false, view: 'library' });
  await page.goto('/');
  await expect(page.locator('.main h1')).toHaveText('推荐');
  expect(await activeSidebar(page)).toEqual(['推荐']);
});

test('新形状往返：切到专辑宫格 → 落盘 page=albums → 刷新仍在专辑', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '专辑' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  await page.waitForTimeout(900); // 防抖
  const stored = await page.evaluate((key) => localStorage.getItem(key), UI_STATE_KEY);
  expect(JSON.parse(stored!)).toMatchObject({ page: 'albums', playlistId: null });
  await page.reload();
  await expect(page.locator('.main h1')).toHaveText('专辑');
});

test('一级路由逐个可达 + 侧栏高亮恰好一个', async ({ page }) => {
  await page.goto('/');
  const cases: Array<{
    via: 'initial' | 'nav' | 'playlist' | 'player';
    text: string;
    title: string;
    /** 侧栏高亮数；默认 1。正在播放页无侧栏席位（docs/25 §6），应为 0 */
    sidebarActive?: number;
  }> = [
    { via: 'initial', text: '', title: '推荐' },
    { via: 'nav', text: '曲库', title: '全部曲目' },
    // docs/28：专辑/艺人升侧栏一级项（曲库 tab 撤掉）
    { via: 'nav', text: '专辑', title: '专辑' },
    { via: 'nav', text: '艺人', title: '艺人' },
    // docs/29 批 A：扫描入库升侧栏一级项（自设置页拆出）
    { via: 'nav', text: '扫描入库', title: '扫描入库' },
    // docs/25 §3：FM 电台回归一级导航（占位页）
    { via: 'nav', text: 'FM 电台', title: 'FM 电台' },
    { via: 'playlist', text: '', title: '' }, // 标题动态取歌单名
    // docs/25 §6：正在播放页从播放条歌曲区域进（侧栏席位已撤）
    { via: 'player', text: '', title: '未在播放', sidebarActive: 0 },
    { via: 'nav', text: '设置', title: '设置' },
  ];

  for (const [i, c] of cases.entries()) {
    if (c.via === 'nav') {
      await page.locator('.sidebar .nav-item', { hasText: c.text }).click();
    } else if (c.via === 'playlist') {
      await page.locator('.sidebar .playlist-item').first().click();
    } else if (c.via === 'player') {
      await page.locator('footer .now').click();
    }
    await expect(page.locator('.main h1').first()).toHaveText(
      c.via === 'playlist'
        ? (await page.locator('.sidebar .playlist-item').first().getAttribute('title'))!.split(' · ')[0]!
        : c.title,
    );
    // 每一步侧栏高亮数符合预期（多高亮=判断写重了，该零的时候非零=残留）
    await expect(page.locator('.sidebar .active')).toHaveCount(c.sidebarActive ?? 1);
    // radio 路由之后落到非列表页，再回列表要能恢复（防 playlistId 残留）
    if (i === cases.length - 1) {
      await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
      await expect(page.locator('.main h1')).toHaveText('全部曲目');
    }
  }
});

test('docs/29 批 A：扫描入库页——多目录 textarea 草稿保存 + 设置页不再含扫描/诊断', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();
  await expect(page.locator('.main h1')).toHaveText('扫描入库');
  // 侧栏高亮恰好「扫描入库」一个
  expect(await activeSidebar(page)).toEqual(['扫描入库']);

  // 多目录 textarea：写入两行 + 一行重复（大小写不同）+ 一空行 → 保存后归一化
  const textarea = page.locator('.roots-input');
  await expect(textarea).toBeVisible();
  await textarea.fill('D:/music/a\nD:/MUSIC/a\n\nD:/music/b');
  await textarea.blur();
  await expect(page.locator('.roots-head .dim')).toContainText('2 个');
  // 归一化结果回填草稿（后端返回为准）
  await expect(textarea).toHaveValue('D:/music/a\nD:/music/b');

  // 设置页不再有「音乐库与任务」「诊断」分区（拆去扫描入库页）
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await expect(page.locator('.main h1')).toHaveText('设置');
  await expect(page.locator('#settings-library')).toHaveCount(0);
  await expect(page.locator('#settings-diagnostics')).toHaveCount(0);
  // 设置页左栏只剩四组
  await expect(page.locator('.settings-rail .rail-item')).toHaveCount(4);
});

test('docs/28：专辑宫格（侧栏一级项）→ 卡片 → 专辑详情 → 返回回宫格', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '专辑' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  // 宫格页与详情页共用「专辑」侧栏席位（高亮跟内容类别走）
  expect(await activeSidebar(page)).toEqual(['专辑']);

  const card = page.locator('.main .k-card').first();
  const albumTitle = await card.locator('.k-card-title').textContent();
  await card.click();

  // 详情页：标题 = 专辑名，meta 带曲目数（B4-3a 聚合的字段）
  await expect(page.locator('.main h1')).toHaveText(albumTitle!.trim());
  await expect(page.locator('.content-header, .main').first()).toContainText(/首/);
  // 详情页上侧栏仍高亮「专辑」
  expect(await activeSidebar(page)).toEqual(['专辑']);
  // 曲目列表是同一查询路径（albumId 筛选），行数 > 0
  await expect(page.locator('.main .list .row').first()).toBeVisible();
  // docs/28：详情页的"专辑"列链接失效（点了=指向自己），只剩歌手列可点
  // （曲库页每行有歌手+专辑两个 .link-cell，这里应只剩 1 个）
  await expect(page.locator('.main .list .row:not(.skeleton):not(.failed)').first().locator('.link-cell')).toHaveCount(1);

  // 返回：回宫格，侧栏高亮不变
  await page.getByRole('button', { name: '返回上一页' }).click();
  await expect(page.locator('.main h1')).toHaveText('专辑');
  expect(await activeSidebar(page)).toEqual(['专辑']);
});

test('docs/28：艺人详情页列出该艺人的曲目，歌手列链接失效', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '艺人' }).click();
  await expect(page.locator('.main h1')).toHaveText('艺人');

  const card = page.locator('.main .k-card').first();
  const artistName = (await card.locator('.k-card-title').textContent())!.trim();
  await card.click();

  await expect(page.locator('.main h1')).toHaveText(artistName);
  // docs/28 修订：艺人不做图片获取逻辑——头部是 user 图标位，不是封面
  await expect(page.locator('.content-header .ch-icon')).toBeVisible();

  // 曲目列表（查询走 artistId 筛选）首行内容含这位艺人——不猜列下标，
  // 列顺序属于 TrackList 的实现细节
  await expect(page.locator('.main .list .row').first()).toContainText(artistName);
});

test('docs/28 修订：曲库列表行内直达详情页，标题显示名称而非路由 id', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  const firstRow = page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
  await firstRow.waitFor({ state: 'visible' });

  // 歌手链接 → 艺人详情页：标题 = 歌手名（回归：真机上曾显示 uuid——
  // 聚合缓存未命中时标题回落到了路由参数；现在回落到列表首行的真名）
  const artistName = await firstRow.locator('.link-cell[title^="查看歌手"]').textContent();
  await firstRow.locator('.link-cell[title^="查看歌手"]').click();
  await expect(page.locator('.main h1')).toHaveText(artistName!.trim());

  // 返回 → 专辑链接 → 专辑详情页：标题 = 专辑名（聚合缓存与首行回落都给同一个名字）
  await page.getByRole('button', { name: '返回上一页' }).click();
  const row = page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
  const albumName = await row.locator('.link-cell[title^="查看专辑"]').textContent();
  await row.locator('.link-cell[title^="查看专辑"]').click();
  await expect(page.locator('.main h1')).toHaveText(albumName!.trim());
  // 专辑详情页保持封面位（封面回落到首曲的 artworkId）
  await expect(page.locator('.content-header .k-cover')).toBeVisible();
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
