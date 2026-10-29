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
    // docs/29 批 A：扫描入库升侧栏一级项（自设置页拆出）
    { via: 'nav', text: '扫描入库', title: '扫描入库' },
    // docs/25 §3：FM 电台回归一级导航（占位页）
    { via: 'nav', text: 'FM 电台', title: 'FM 电台' },
    { via: 'playlist', text: '', title: '' }, // 标题动态取歌单名
    // docs/25 §6：正在播放页从播放条歌曲区域进（侧栏席位已撤）
    { via: 'player', text: '', title: '未在播放', sidebarActive: 0 },
    { via: 'nav', text: '设置', title: '设置' }, // 2026-10-28 三次修订：单页平铺，标题固定
  ];

  for (const [i, c] of cases.entries()) {
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
    // radio 路由之后落到非列表页，再回列表要能恢复（防 playlistId 残留）
    if (i === cases.length - 1) {
      await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
      await expect(page.locator('.main h1')).toHaveText('全部曲目');
    }
  }
});

test('docs/29 批 A 六轮：扫描入库页——目录条目列表增删 + 设置页不再含扫描/诊断', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();
  await expect(page.locator('.main h1')).toHaveText('扫描入库');
  // 侧栏高亮恰好「扫描入库」一个
  expect(await activeSidebar(page)).toEqual(['扫描入库']);

  // 目录是条目列表（textarea 已废）：mock 默认 1 个根
  const list = page.locator('.root-list');
  await expect(list.locator('.root-row')).toHaveCount(1);

  // 点移除 → 内联确认条报影响面（mock 每目录示意 3 首）→ 仅停止监听
  await list.locator('.root-row').first().getByRole('button').click();
  const confirm = page.locator('.roots-confirm');
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText('涉及 3 首曲目');
  await confirm.locator('button', { hasText: '仅停止监听' }).click();
  // 列表清空 → 空态提示（七轮起没有"N 个目录"计数行）
  await expect(list).toHaveCount(0);
  await expect(page.locator('.roots-empty')).toBeVisible();

  // 浏览器里没有系统目录选择器：添加目录 = 用户取消，列表不动、无报错
  await page.getByRole('button', { name: '添加目录' }).click();
  await expect(page.locator('.roots-empty')).toBeVisible();
  await expect(page.locator('.tool-error')).toHaveCount(0);

  // 设置页不再有「音乐库与任务」「诊断」分区（拆去扫描入库页）。
  // 2026-10-28 三次修订：改回单页平铺——大标题固定「设置」，tab 只做跳转，
  // 目录条目列表只存在于扫描入库页
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await expect(page.locator('.main h1')).toHaveText('设置');
  await expect(page.locator('.settings-tabs .settings-tab')).toHaveCount(5);
  await expect(page.locator('.root-list')).toHaveCount(0);
});

test('docs/29 批 A 八轮：立即索引跑完 → 执行状态留下上次记录', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  // 初始无记录（七轮起执行状态并入曲库卡片，用 status-block 定位）
  const card = page.locator('#tools-library .status-block');
  await expect(card).toContainText('还没有索引记录');

  // 立即索引：mock 走完 walking→parsing→importing→done，收尾后状态留记账
  await page.getByRole('button', { name: '立即索引' }).click();
  await expect(card).toContainText('上次索引：', { timeout: 15_000 });
  await expect(card).toContainText('新增');
  // 自动刷新行已删（八轮）：静默管线不再占版面
  await expect(card).not.toContainText('自动刷新');
});

test('docs/40 十三轮：整理输出根 radio（回退首根）+ 待整理目录未设置引导', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();
  await page.locator('.root-list .root-row').first().waitFor();

  // 输出根 radio：mock 只有 1 个根，"没勾选回退首根"让它显示为选中
  const radio = page.locator('.root-row .root-target input[type="radio"]');
  await expect(radio).toBeVisible();
  await expect(radio).toBeChecked();

  // 整理库段：待整理目录未设置 → 引导文案 + 选择按钮。
  // 点它 = 系统对话框被浏览器取消（mock chooseRoot 回 null）→ 状态不动、无报错
  const inboxRow = page.locator('#tools-beets .inbox-row');
  await expect(inboxRow).toContainText('还没设置');
  await inboxRow.getByRole('button', { name: '选择待整理目录' }).click();
  await expect(inboxRow).toContainText('还没设置');
  await expect(page.locator('.import-error')).toHaveCount(0);
});

test('docs/29 批 A 八轮：取消任务停在 cancelled 终态（预览 mock 实现真取消）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  // 发起索引后立刻取消（mock 每步 400ms，点击必落在扫描中途）
  await page.getByRole('button', { name: '立即索引' }).click();
  const cancel = page.locator('#tools-library .status-line button', { hasText: '取消任务' });
  await expect(cancel).toBeVisible();
  await cancel.click();

  // 终态：不再转圈，留下"已取消"记账；重新可点「立即索引」
  const card = page.locator('#tools-library .status-block');
  await expect(card).toContainText('上次索引已取消', { timeout: 10_000 });
  await expect(card.locator('.spinning')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '立即索引' })).toBeEnabled();
});

test('docs/29 批 A 八轮：曲库页扫描条只做指示，不再有第二个取消按钮', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();

  // 在扫描入库页发起扫描 → 切到曲库页看环境指示条
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();
  await page.getByRole('button', { name: '立即索引' }).click();
  await page.locator('.sidebar .nav-item', { hasText: '曲库' }).click();
  const bar = page.locator('.scan-bar');
  await expect(bar).toBeVisible();
  await expect(bar.getByRole('button')).toHaveCount(0);

  // 等扫描自然结束，指示条消失
  await expect(bar).toBeHidden({ timeout: 15_000 });
});

test('扫描入库页只剩两组卡片；识别信息与最近错误在设置页「关于」', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  // 两组：曲库（必有）与整理库（beets，可选）。以前是七个并列段落。
  await expect(page.locator('#tools-library .section-heading h2')).toHaveText('曲库');
  await expect(page.locator('#tools-beets .section-heading h2')).toHaveText('整理库');

  // 高级诊断折叠区已整体迁去设置页「关于」（2026-10-28 用户拍板）：
  // 扫描入库页不再有 details 折叠、也不再能找到版本/错误区
  await expect(page.locator('.advanced-fold')).toHaveCount(0);
  await expect(page.locator('.diag-grid')).toHaveCount(0);

  // 设置 → 关于：版本 + 数据目录 + 最近错误（单页平铺，tab 点击只滚动）。
  // 2026-10-28 九轮半拍板：一行一项；平台/Schema/播放列表/重新读取已删；
  // 版本行带禁用的「获取更新」占位按钮；爱发电二维码在支持开发行
  await page.locator('.sidebar .nav-item', { hasText: '设置' }).click();
  await page.locator('.settings-tab', { hasText: '关于' }).click();
  await expect(page.locator('.main h1')).toHaveText('设置');
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

  // 清空播放历史按钮随迁到扫描入库页曲库卡的动作行
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();
  await expect(page.getByRole('button', { name: '清空播放历史' })).toBeVisible();
});

test('beets 未装时整理库段收成一行，且不拖累曲库段（docs/35 P2-11）', async ({ page }) => {
  // `?beets=absent` 让 mock 的 beets_status 报 found:false。这条路径以前只有肉眼验过：
  // mock 里 found 恒 true，所以"未装"这一屏从来没有测试断言过。
  await page.goto('/?beets=absent');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  const beets = page.locator('#tools-beets');
  // 收拢形态：一行说明 + 两个安装按钮，其余什么都没有
  await expect(beets).toContainText('整理库未启用');
  await expect(beets.getByRole('button', { name: /快速安装/ })).toBeVisible();
  await expect(beets.getByRole('button', { name: /免安装包/ })).toBeVisible();

  // 导入面板与维护四件套必须整块不存在（docs/33 的原则：不是"禁用"，是不该出现）
  await expect(beets.getByRole('heading', { name: '导入新文件夹' })).toHaveCount(0);
  await expect(beets.locator('.maint-fold')).toHaveCount(0);
  await expect(beets.getByRole('button', { name: '登记到整理库' })).toHaveCount(0);

  // 曲库段不受影响：页面上要能看出"没装 beets 也不影响播放"
  await expect(page.locator('#tools-library .section-heading h2')).toHaveText('曲库');
  await expect(page.locator('#tools-library')).toContainText('曲目');
});

test('docs/29 批 D：整理库四件套（登记/重写标签/查重/重排文件）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  // 四件套收在折叠区里（默认收起）：它们是对已有曲目做批量操作，
  // 不是每天要做的事。要用先展开。
  // （市场折叠区也用 maint-fold 样式，所以这里按 summary 文案锚定）
  await page.locator('.maint-fold > summary', { hasText: '整理已有曲目' }).click();

  const card = page.locator('.maint');
  await expect(card).toBeVisible();
  // 目标根来自 beets 状态（config.yaml 显式配置的那个），不是猜的 ~/Music
  await expect(card.locator('.task-status-head .dim')).toContainText('目标根 D:/MUSIC_Player/test-music');

  // 登记到整理库：事件流走到 adoptDone，汇总行显示登记数
  await card.getByRole('button', { name: '登记到整理库' }).click();
  await expect(card.locator('.maint-line')).toContainText('登记 3 首', { timeout: 10_000 });

  // 重写文件标签：先预览出 diff（预览态才出现「应用这些修改」）
  await card.getByRole('button', { name: '重写文件标签' }).click();
  await expect(card.locator('.maint-line')).toContainText('待更新 3 首', { timeout: 10_000 });
  await expect(card.locator('.maint-diff .chip').first()).toContainText('title');
  const apply = card.getByRole('button', { name: '应用这些修改' });
  await expect(apply).toBeVisible();
  await apply.click();
  await expect(card.locator('.maint-line')).toContainText('已更新 3 首', { timeout: 10_000 });
  // 应用完成后预览按钮消失（不留一个会重复写库的入口）
  await expect(card.getByRole('button', { name: '应用这些修改' })).toHaveCount(0);

  // 查找重复：只读分组 + 每首带「定位」
  await card.getByRole('button', { name: '查找重复' }).click();
  await expect(card.locator('.maint-line')).toContainText('1 组重复', { timeout: 10_000 });
  await expect(card.locator('.maint-group-head .chip')).toHaveText(/\S/);
  await expect(card.locator('.maint-sublist .link').first()).toHaveText('定位');
  // 应用内不提供删除入口（危险操作留给 beets CLI / 资源管理器）
  await expect(card.getByRole('button', { name: /删除/ })).toHaveCount(0);

  // 重排文件位置：预览出「从哪到哪」清单，确认后才真搬
  await card.getByRole('button', { name: '重排文件位置' }).click();
  await expect(card.locator('.maint-line')).toContainText('将移动 3 首', { timeout: 10_000 });
  const rows = card.locator('.maint-list > li');
  await expect(rows).toHaveCount(3);
  await expect(rows.first()).toContainText('→');
  await card.getByRole('button', { name: '确认移动' }).click();
  await expect(card.locator('.maint-line')).toContainText('已移动 3 首', { timeout: 10_000 });
});

test('docs/40：插件市场（展开懒加载 → 白名单三态 → 安装事件流翻绿）', async ({ page }) => {
  await page.goto('/');
  await page.locator('.sidebar .nav-item', { hasText: '扫描入库' }).click();

  // 市场折叠区（默认收起）：目录是一次性短进程查询，展开才拉——
  // 这不是缓存优化，是"别让每次进页都白起一个进程"。
  const market = page.locator('.market-fold');
  await market.locator('summary').click();

  // 白名单目录：mock 给 3 项覆盖三种状态；builtin 档没有安装按钮
  await expect(market.locator('.market-list > li')).toHaveCount(3);
  const fetchart = market.locator('.market-list > li', { hasText: 'fetchart' });
  await expect(fetchart.locator('.badge')).toHaveText('缺依赖');
  await expect(fetchart.getByRole('button', { name: '安装' })).toBeVisible();
  await expect(
    market.locator('.market-list > li', { hasText: 'replaygain' }).getByRole('button', { name: '安装' }),
  ).toHaveCount(0);

  // 安装：pluginProgress 逐行走 → pluginDone 带复检报告 → 徽标翻「可用」
  await fetchart.getByRole('button', { name: '安装' }).click();
  await expect(market.locator('.market-progress')).toContainText('安装完成', { timeout: 10_000 });
  await expect(fetchart.locator('.badge')).toHaveText('可用');
  // 装完安装按钮消失（复检已就绪，不再给重复安装入口）
  await expect(fetchart.getByRole('button', { name: '安装' })).toHaveCount(0);
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

  // 上限单独一行；排序不再出现在编辑器里（由列表列头接管）
  await dialog.locator('input[type="number"]').fill('10');
  await expect(dialog.locator('.rule-meta [role="combobox"]')).toHaveCount(0);

  // 创建 → 侧栏出现带 activity 图标徽标的歌单（2026-10-30：智字换图标）
  await dialog.locator('.prompt-body input[type="text"]').first().fill('我的流行电台');
  await dialog.getByRole('button', { name: '创建' }).click();
  await expect(dialog).toHaveCount(0);
  const item = page.locator('.sidebar .playlist-item', { hasText: '我的流行电台' });
  await expect(item).toHaveCount(1);
  await expect(item.locator('.pl-badge svg')).toBeVisible();
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
