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

test('旧 UiState：home 且无 libraryDetail → 推荐页（仪表盘）', async ({ page }) => {
  await seedState(page, { page: 'home', libraryDetail: false, view: 'library' });
  await page.goto('/');
  // 2026-10-28：推荐页仪表盘化，没有页标题
  await expect(page.locator('.main h1')).toHaveCount(0);
  await expect(page.locator('.main .playlist-row .playlist-card').first()).toBeVisible();
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
    { via: 'initial', text: '', title: '' }, // 仪表盘无页标题（2026-10-28）
    { via: 'nav', text: '曲库', title: '全部曲目' },
    // docs/28：专辑/艺人升侧栏一级项（曲库 tab 撤掉）
    { via: 'nav', text: '专辑', title: '专辑' },
    { via: 'nav', text: '艺人', title: '艺人' },
    // docs/48：beets 整理库独立成页
    { via: 'nav', text: '整理库', title: '整理库' },
    // docs/25 §3：FM 电台回归一级导航（占位页）
    { via: 'nav', text: 'FM 电台', title: 'FM 电台' },
    { via: 'playlist', text: '', title: '' }, // 标题动态取歌单名
    // 2026-10-30：原「扫描入库」一级页并入设置页「曲库」组，侧栏席位撤销
    // （下面「底部工具组只有两项」那条用例守住"它不能复活"）
    { via: 'nav', text: '设置', title: '设置' }, // 2026-10-28 三次修订：单页平铺，标题固定
    // **正在播放页放最后**：那一页侧栏整体退出（docs/46 §5.2），留在它上面时
    // 后面任何一次 `.sidebar .nav-item` 点击都会"element is not visible"。
    // 这条顺序不是随意的——它曾经排在中间，于是这个用例稳定超时 30s。
    // docs/25 §6：正在播放页从播放条歌曲区域进（侧栏席位已撤）
    { via: 'player', text: '', title: '未在播放', sidebarActive: 0 },
  ];

  for (const c of cases) {
    if (c.via === 'nav') {
      await page.locator('.sidebar .nav-item', { hasText: c.text }).click();
    } else if (c.via === 'playlist') {
      await page.locator('.sidebar .playlist-item').first().click();
    } else if (c.via === 'player') {
      await page.locator('footer .now').click();
    }
    if (c.via === 'initial') {
      // 仪表盘：无 h1，以歌单卡片行认页面
      await expect(page.locator('.main h1')).toHaveCount(0);
      await expect(page.locator('.main .playlist-row .playlist-card').first()).toBeVisible();
    } else {
      await expect(page.locator('.main h1').first()).toHaveText(
        c.via === 'playlist'
          ? (await page.locator('.sidebar .playlist-item').first().getAttribute('title'))!.split(' · ')[0]!
          : c.title,
      );
    }
    // 每一步侧栏高亮数符合预期（多高亮=判断写重了，该零的时候非零=残留）
    await expect(page.locator('.sidebar .active')).toHaveCount(c.sidebarActive ?? 1);
    // 整页之后回列表要能恢复（防 playlistId 残留）。挂在「设置」这一步：
    // 它现在是最后一个**有侧栏**的页面（正在播放页侧栏退出，回不去列表）
    if (c.via === 'nav' && c.text === '设置') {
      await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
      await expect(page.locator('.main h1')).toHaveText('全部曲目');
    }
  }
});

/**
 * 打开「设置 → 曲库」组。
 *
 * 2026-10-30：原「扫描入库」一级页并入设置页，下面六条用例（原本都以
 * `.sidebar .nav-item 扫描入库` 起步）统一改走这条路。断言主体没变——
 * 目录条目列表、执行状态卡片、动作按钮都还在，只是换了个宿主。
 */
async function openLibrarySettings(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await page.locator('.settings-tab', { hasText: '曲库' }).click();
  await expect(page.locator('.main h1')).toHaveText('设置');
}

test('2026-10-30：侧栏底部工具组只剩「整理库 / 设置」两项', async ({ page }) => {
  await page.goto('/');
  // 「扫描入库」那条路走完了：docs/29 批 A 升成一级页 → docs/48 拆走 beets →
  // 只剩一条目录卡片 → 并入设置页「曲库」组。席位留着就是死链接。
  await expect(page.locator('.sidebar .nav-item', { hasText: '扫描入库' })).toHaveCount(0);
  const footer = page.locator('.sidebar .sidebar-footer .nav-item');
  await expect(footer).toHaveCount(2);
  await expect(footer.nth(0)).toHaveText('整理库');
  await expect(footer.nth(1)).toHaveText('设置');
});

test('2026-10-30：首页「位置缺失」告警 → 设置页曲库组（滚过去 + 高亮那一组）', async ({
  page,
}) => {
  // `?missing=2` 让 mock 的 library_stats 报出缺失（默认 0，那条告警不出现）。
  // 这条路径以前指向「扫描入库」页；那页并入设置页之后，导航变成"切路由 +
  // 一次性跳转请求"，只切路由的话人会落在「外观」组——正是这次要守的东西。
  await page.goto('/?missing=2');
  const warn = page.locator('.stat-warn');
  await expect(warn).toContainText('2 个位置缺失');
  await expect(warn).toContainText('设置 → 曲库');
  await warn.click();

  await expect(page.locator('.main h1')).toHaveText('设置');
  // 落在曲库组：tab 高亮 + 该组滚进视野。
  // 用 poll 而不是一次性取值：滚动是 `behavior:'smooth'`，跨一屏的距离要几百毫秒。
  await expect(page.locator('.settings-tab.on')).toHaveText('曲库');
  await expect
    .poll(
      () => page.locator('#settings-library').evaluate((el) => el.getBoundingClientRect().top),
      { timeout: 5_000 },
    )
    .toBeLessThan(300);
  // 侧栏高亮仍只有「设置」一个（曲库组不是路由）
  expect(await activeSidebar(page)).toEqual(['设置']);
});

test('docs/29 批 A 六轮：曲库目录条目列表增删（设置 → 曲库）', async ({ page }) => {
  await openLibrarySettings(page);
  // 曲库组不是独立路由：侧栏高亮只有「设置」一个
  expect(await activeSidebar(page)).toEqual(['设置']);

  // 目录是条目列表（textarea 已废）：mock 默认 1 个根
  const list = page.locator('#settings-library .root-list');
  await expect(list.locator('.root-row')).toHaveCount(1);

  // 点移除 → 内联确认条报影响面（mock 每目录示意 3 首）→ 仅停止监听
  await list.locator('.root-row').first().getByRole('button').click();
  const confirm = page.locator('#settings-library .roots-confirm');
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText('涉及 3 首曲目');
  await confirm.locator('button', { hasText: '仅停止监听' }).click();
  // 列表清空 → 空态提示（七轮起没有"N 个目录"计数行）
  await expect(list).toHaveCount(0);
  await expect(page.locator('#settings-library .roots-empty')).toBeVisible();

  // 浏览器里没有系统目录选择器：添加目录 = 用户取消，列表不动、无报错
  await page.getByRole('button', { name: '添加目录' }).click();
  await expect(page.locator('#settings-library .roots-empty')).toBeVisible();
  await expect(page.locator('#settings-library .tool-error')).toHaveCount(0);
});

test('docs/29 批 A 八轮：立即索引跑完 → 执行状态留下上次记录', async ({ page }) => {
  await openLibrarySettings(page);

  // 初始无记录（七轮起执行状态并入曲库卡片，用 status-block 定位）
  const card = page.locator('#settings-library .status-block');
  await expect(card).toContainText('还没有索引记录');

  // 立即索引：mock 走完 walking→parsing→importing→done，收尾后状态留记账
  await page.getByRole('button', { name: '立即索引' }).click();
  await expect(card).toContainText('上次索引：', { timeout: 15_000 });
  await expect(card).toContainText('新增');
  // 自动刷新行已删（八轮）：静默管线不再占版面
  await expect(card).not.toContainText('自动刷新');
});

test('docs/48：曲库目录行不再带 beets 的「整理输出」单选', async ({ page }) => {
  await openLibrarySettings(page);
  await page.locator('#settings-library .root-list .root-row').first().waitFor();

  // 2026-10-30 解耦：这个 radio 以前直接写 beets 的 config.yaml——曲库是
  // **本应用**的设置面，不该出现另一个库的配置项。搬走之后若有人"顺手加回来"，
  // 症状是曲库又开始改一个它不拥有的文件。
  await expect(page.locator('.root-list .root-target')).toHaveCount(0);
  await expect(page.locator('input[name="beets-target-root"]')).toHaveCount(0);

  // 目录行本身还在（路由与移除按钮不受影响）
  await expect(page.locator('.root-list .root-path').first()).not.toBeEmpty();
});

test('docs/29 批 A 八轮：取消任务停在 cancelled 终态（预览 mock 实现真取消）', async ({ page }) => {
  await openLibrarySettings(page);

  // 发起索引后立刻取消（mock 每步 400ms，点击必落在扫描中途）
  await page.getByRole('button', { name: '立即索引' }).click();
  const cancel = page.locator('#settings-library .status-line button', { hasText: '取消任务' });
  await expect(cancel).toBeVisible();
  await cancel.click();

  // 终态：不再转圈，留下"已取消"记账；重新可点「立即索引」
  const card = page.locator('#settings-library .status-block');
  await expect(card).toContainText('上次索引已取消', { timeout: 10_000 });
  await expect(card.locator('.spinning')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '立即索引' })).toBeEnabled();
});

test('docs/29 批 A 八轮：曲库页扫描条只做指示，不再有第二个取消按钮', async ({ page }) => {
  await page.goto('/');
  // 在设置页「曲库」组发起扫描 → 切到曲库页看环境指示条
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await page.locator('.settings-tab', { hasText: '曲库' }).click();
  await page.getByRole('button', { name: '立即索引' }).click();
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  const bar = page.locator('.scan-bar');
  await expect(bar).toBeVisible();
  await expect(bar.getByRole('button')).toHaveCount(0);

  // 等扫描自然结束，指示条消失
  await expect(bar).toBeHidden({ timeout: 15_000 });
});

test('设置页「曲库」组：统计卡 + 曲库卡片；beets 段与诊断折叠区都不在这', async ({ page }) => {
  await openLibrarySettings(page);

  // 六张统计卡（曲目/专辑/艺人/总时长/文件容量/索引大小；位置缺失为 0 时不出现）。
  // 2026-10-30 补第六项：五项时 3 × 2 的第二行右边空一块，看着像布局没做完。
  const cards = page.locator('#settings-library .stat-card');
  await expect(cards).toHaveCount(6);
  await expect(cards.first()).toContainText('曲目');
  // 文件容量 ≠ 索引大小：前者是音乐文件加起来多大，后者是 SQLite 自己的体积
  await expect(page.locator('#settings-library .stat-card', { hasText: '文件容量' })).toHaveCount(1);
  await expect(page.locator('#settings-library .stat-card', { hasText: '索引大小' })).toHaveCount(1);

  // 整页六个 tab（曲库/外观/播放/系统/快捷键/关于）——2026-10-30 曲库提到第一位
  await expect(page.locator('.settings-tabs .settings-tab')).toHaveCount(6);

  // 整理库（beets）在 2026-10-30 已搬成侧栏独立一页（docs/48）
  await expect(page.locator('#tools-beets')).toHaveCount(0);
  await expect(page.locator('.beets-section')).toHaveCount(0);

  // 高级诊断折叠区已整体迁去「关于」：曲库组不再有 details 折叠、也找不到版本/错误区
  await expect(page.locator('.advanced-fold')).toHaveCount(0);
  await expect(page.locator('.diag-grid')).toHaveCount(0);

  // 设置 → 关于：版本 + 数据目录 + 最近错误（单页平铺，tab 点击只滚动）。
  // 2026-10-28 九轮半拍板：一行一项；平台/Schema/播放列表/重新读取已删；
  // 版本行带禁用的「获取更新」占位按钮；爱发电二维码在支持开发行
  await page.locator('.settings-tab', { hasText: '关于' }).click();
  const about = page.locator('.settings-about');
  await expect(about.locator('.setting-row', { hasText: '版本' })).toContainText('0.1.0');
  await expect(about.getByRole('button', { name: '获取更新' })).toBeDisabled();
  await expect(about.locator('.setting-row', { hasText: '数据目录' })).toBeVisible();
  await expect(about.getByRole('button', { name: '打开目录' })).toBeVisible();
  await expect(about.locator('img.afdian-qr')).toBeVisible();
  await expect(about.locator('.diag-block')).toContainText('最近错误');
  // 删掉的三项不能复活
  await expect(about).not.toContainText('平台');
  await expect(about).not.toContainText('Schema');
  await expect(about.getByRole('button', { name: '重新读取' })).toHaveCount(0);

  // 清空播放历史按钮随迁在曲库卡的动作行
  await page.locator('.settings-tab', { hasText: '曲库' }).click();
  await expect(page.getByRole('button', { name: '清空播放历史' })).toBeVisible();
});

test('docs/48：整理库是侧栏独立的一页（可达 + 高亮恰好一个）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();
  await expect(page.locator('.main h1')).toHaveText('整理库');
  expect(await activeSidebar(page)).toEqual(['整理库']);
  // 装了组件就有查询区——这一页的主体就是查询
  await expect(page.getByRole('button', { name: '查询' })).toBeVisible();
});

test('docs/48：未装 beets 时整理库页只给安装引导，不出查询区', async ({ page }) => {
  // `?beets=absent` 让 mock 的 beets_status 报 found:false。
  // 这条路径以前只有肉眼验过（mock 里 found 恒 true）。
  await page.goto('/?beets=absent');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();

  const main = page.locator('.main');
  await expect(main).toContainText('整理库未启用');
  // 两条路径并排（docs/51 §4）：差别的全部就是"有没有本机 Python"
  await expect(main.getByRole('button', { name: /下载并安装/ })).toBeVisible();
  await expect(main).toContainText('本机 Python');
  // mock 的 Python 探测报"找不到" → 那条只给"重新检测"，不给安装按钮
  // （免得用户点了才发现没有 Python）
  await expect(main.getByRole('button', { name: /重新检测/ })).toBeVisible();
  await expect(main.getByRole('button', { name: /安装（\d+ 个插件）/ })).toHaveCount(0);

  // 没装就没有查询入口。导入面板 / 维护四件套 / 插件市场都没搬过来
  // （docs/33 的原则：不该出现的整块不要出现，不是"禁用"）
  await expect(main.getByRole('button', { name: '查询' })).toHaveCount(0);
  await expect(main.getByRole('heading', { name: '导入新文件夹' })).toHaveCount(0);
  await expect(main.locator('.maint-fold')).toHaveCount(0);
  await expect(main.locator('.market-fold')).toHaveCount(0);
});

test('docs/51：安装页给出插件勾选与卸载入口（卸载只删组件）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();
  await page.locator('.main .pt-tab', { hasText: '安装' }).click();

  const main = page.locator('.main');
  // 内嵌插件表不依赖已装组件（`beets_plugin_table` 读的是编译期 JSON），
  // 所以勾选清单现在就应该在——否则用户没法在安装前决定要什么
  await expect(main.locator('.pick input[type="checkbox"]')).toHaveCount(4);
  await expect(main).toContainText('beautifulsoup4>=4.12,<5');
  await expect(main).toContainText('另下 fpcalc');

  // 卸载是两段式：先「卸载组件」，点了才出「确认卸载」。
  // 破坏性操作不该一次点击就执行
  await expect(main.getByRole('button', { name: '确认卸载' })).toHaveCount(0);
  await main.getByRole('button', { name: '卸载组件' }).click();
  await expect(main.getByRole('button', { name: '确认卸载' })).toBeVisible();
  // 文案必须写明数据不动——这是卸载唯一容易误解的地方
  await expect(main).toContainText('整理库与分析库不动');
});

test('docs/48：查询 → 未入库行不可选 → 全选存为歌单', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();

  // 不加条件 = 全库。mock 在末尾挂了一条"只在整理库里有"的条目，
  // 用来演"未入库"这一行——那条分支否则只有真机能看见
  await page.getByRole('button', { name: '查询' }).click();
  const rows = page.locator('.result-panel .row');
  await expect(rows.first()).toBeVisible();

  const missing = page.locator('.result-panel .row.missing');
  await expect(missing).toHaveCount(1);
  await expect(missing).toContainText('未入库');
  // 未入库的行勾不了：勾了也进不去歌单，那种"能点但无效"是最差的形态
  await expect(missing.getByRole('checkbox')).toBeDisabled();

  // 全选只覆盖已入库的行
  await page.getByRole('checkbox', { name: '全选本页已入库的曲目' }).click();
  const total = await rows.count();
  await expect(page.locator('.result-bar')).toContainText(`已选 ${total - 1} 首`);

  // 存为歌单：同名合并（后端按名字派生 id），所以不会建出第二份
  await page.getByRole('button', { name: '存为歌单' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.locator('input[type="text"]').fill('整理库精选');
  await dialog.getByRole('button', { name: '保存' }).click();

  await expect(page.locator('.k-toast')).toContainText('已存为歌单');
  await expect(page.locator('.sidebar .playlist-item', { hasText: '整理库精选' })).toHaveCount(1);
  // 存完清空选择、按钮回到禁用（防重复点出一份一样的）
  await expect(page.getByRole('button', { name: '存为歌单' })).toBeDisabled();
});

test('docs/49：整理库拆成 安装/索引/整理 三个子页，各自只放自己的东西', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();

  const main = page.locator('.main');
  const tabs = main.locator('.pt-tab');
  await expect(tabs).toHaveCount(3);
  await expect(tabs.nth(0)).toHaveText('安装');
  await expect(tabs.nth(1)).toHaveText('索引');
  await expect(tabs.nth(2)).toHaveText('整理');

  // 默认停在「索引」：装好的人占多数，每次先看安装说明是无用的
  await expect(tabs.nth(1)).toHaveClass(/active/);
  await expect(main.getByRole('button', { name: '查询' })).toBeVisible();
  await expect(main).not.toContainText('待整理目录');

  // 安装页：组件状态是这一页的主体，且不该再出现查询区
  await tabs.nth(0).click();
  await expect(main).toContainText('组件状态');
  await expect(main).toContainText('入口来源');
  await expect(main.getByRole('button', { name: '查询' })).toHaveCount(0);

  // 整理页：三个目录各一张卡，读（查询）在这里不该出现
  await tabs.nth(2).click();
  await expect(main).toContainText('待整理目录');
  await expect(main).toContainText('整理落点');
  await expect(main.getByRole('button', { name: '查询' })).toHaveCount(0);
});

test('docs/49：整理链路 待整理目录 → 预览 → 应用所选 → 汇总', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '整理库' }).click();
  await page.locator('.main .pt-tab', { hasText: '整理' }).click();

  const main = page.locator('.main');

  // 没设待整理目录 → 「开始整理」禁用（按钮在，但按不动；不是消失）
  const start = main.getByRole('button', { name: /开始整理/ });
  await expect(start).toBeDisabled();

  // 选目录走 pick_folder（纯选择器，不写曲库根）；mock 回一个曲库外的路径。
  // 两张卡（待整理 / 落点）都会出现路径，所以定位取第一张卡的
  await main.getByRole('button', { name: '选择待整理目录' }).click();
  await expect(main.locator('.beets-path').first()).toContainText('inbox');
  await expect(start).toBeEnabled();

  // 预览：mock 发两张专辑（一张高置信默认选中、一张低置信默认跳过）
  await start.click();
  await expect(main.locator('.album-card')).toHaveCount(2);
  // 低置信不默认勾选（docs/08 §4.1）：所以只有 1 张会被写
  await expect(main.getByRole('button', { name: /应用所选（1 张）/ })).toBeVisible();

  // 应用：走 applied → applyDone，汇总出现且按钮解锁
  await main.getByRole('button', { name: /应用所选/ }).click();
  await expect(main).toContainText('已应用 1 张');
  await expect(main).toContainText('跳过 1 张');
});

test('docs/40 二轮：动态歌单模态框（Switch → 条件模式行 → 创建动态歌单）', async ({ page }) => {
  await page.goto('/');

  // 侧栏「歌单」标题旁的新建入口 → 统一模态框（无描述行，类型是开关）
  await page.getByRole('button', { name: '新建歌单' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // 动态歌单 Switch：关 = 手动，开 = 智能并展开规则编辑器
  const dynSwitch = dialog.getByRole('switch', { name: '动态歌单' });
  await expect(dynSwitch).toBeVisible();
  await expect(dialog.locator('.rule-editor')).toHaveCount(0);
  await dynSwitch.click();
  await expect(dynSwitch).toHaveAttribute('aria-checked', 'true');
  await expect(dialog.locator('.rule-editor')).toBeVisible();

  // 条件行：kit Select（combobox trigger + portal 选项列表）两列
  await dialog.getByRole('button', { name: '+ 添加条件' }).click();
  const row = dialog.locator('.rule-row').first();
  await row.getByRole('combobox', { name: '条件' }).click();
  await page.getByRole('option', { name: '流派' }).click();
  await row.getByRole('combobox', { name: '流派' }).click();
  await page.getByRole('option', { name: '流行' }).click();

  // 上限输入已随物化模型移除（2026-10-30）；排序在编辑器里不出现
  await expect(dialog.locator('input[type="number"]')).toHaveCount(0);
  await expect(dialog.locator('.rule-meta [role="combobox"]')).toHaveCount(0);

  // 创建 → 侧栏出现带 activity 图标徽标的歌单（2026-10-30：智字换图标）
  await dialog.locator('.prompt-body input[type="text"]').first().fill('我的流行电台');
  await dialog.getByRole('button', { name: '创建' }).click();
  await expect(dialog).toHaveCount(0);
  const item = page.locator('.sidebar .playlist-item', { hasText: '我的流行电台' });
  await expect(item).toHaveCount(1);
  await expect(item.locator('.pl-badge svg')).toBeVisible();

  // 物化模型（2026-10-30）：建单即按规则入库 → 打开有行；头部有"刷新"按钮，
  // 无新命中时刷新提示"没有新增"
  await item.click();
  await expect(page.locator('.main h1')).toHaveText('我的流行电台');
  await expect(page.locator('.main .list .row').first()).toBeVisible();
  await page.getByRole('button', { name: '刷新' }).click();
  await expect(page.locator('.k-toast')).toContainText('没有新增');
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
  // 2026-10-28：曲库三 tab 不出现在搜索页（那是曲库页的子状态）
  await expect(page.locator('.lib-tabs')).toHaveCount(0);
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

test('docs/27 五轮：筛选器字段条件（kit Select/Field 表单）应用 → 角标 → 空态 → 清除恢复', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  await expect(page.locator('.main h1')).toHaveText('全部曲目');
  const firstRow = page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
  await firstRow.waitFor({ state: 'visible' });

  // 打开筛选器 → 流派选"流行"（mock 种子池五选一，部分命中 → 列表仍在）
  await page.getByRole('button', { name: '筛选器', exact: true }).click();
  const dialog = page.getByRole('dialog').filter({ hasText: '筛选曲目' });
  await dialog.getByRole('combobox', { name: '流派' }).click();
  await page.getByRole('option', { name: '流行' }).click();
  await dialog.getByRole('button', { name: '应用' }).click();

  // 行还在（部分命中），筛选按钮带 1 档生效角标
  await firstRow.waitFor({ state: 'visible' });
  await expect(page.getByRole('button', { name: /筛选器（1 档生效）/ })).toBeVisible();

  // 重开时已生效条件回填到字段（触发按钮显示所选流派）
  await page.getByRole('button', { name: /筛选器（1 档生效）/ }).click();
  await expect(dialog.getByRole('combobox', { name: '流派' })).toContainText('流行');

  // 收紧到必然 0 命中：年份区间 1800–1801（mock 年份 2015 起）→ 空态是
  // "筛选后没有匹配"，不是"曲库还是空的"
  await dialog.getByRole('spinbutton', { name: '年份不早于' }).fill('1800');
  await dialog.getByRole('spinbutton', { name: '年份不晚于' }).fill('1801');
  await dialog.getByRole('button', { name: '应用' }).click();
  // 空态已统一到 EmptyState 组件（.k-empty）
  await expect(page.locator('.main .k-empty')).toContainText('筛选后没有匹配的曲目');

  // 空态里的「清除筛选」一键恢复 → 列表回来、角标消失
  await page.getByRole('button', { name: '清除筛选', exact: true }).click();
  await firstRow.waitFor({ state: 'visible' });
  await expect(page.getByRole('button', { name: '筛选器', exact: true })).toBeVisible();
});

test('docs/27 五轮：筛选器全部留空 = 不过滤（无角标，列表不受影响）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  const firstRow = page.locator('.main .list .row:not(.skeleton):not(.failed)').first();
  await firstRow.waitFor({ state: 'visible' });

  await page.getByRole('button', { name: '筛选器', exact: true }).click();
  const dialog = page.getByRole('dialog').filter({ hasText: '筛选曲目' });
  // 固定字段表单里"留空"就是"该字段不过滤"，全空应用 = 无条件生效，
  // 不再需要旧动态行的"空值拦截"报错
  await dialog.getByRole('button', { name: '应用' }).click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole('button', { name: '筛选器', exact: true })).toBeVisible();
  await expect(firstRow).toBeVisible();
});
