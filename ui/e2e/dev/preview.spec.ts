import { expect, test, type Page } from '@playwright/test';

const runtimeErrors = new WeakMap<Page, string[]>();

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  runtimeErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() !== 'error') return;
    if (message.location().url.endsWith('/auth/login') && message.text().includes('401')) return;
    errors.push(message.text());
  });
});

test.afterEach(async ({ page }) => {
  expect(runtimeErrors.get(page)).toEqual([]);
});

test('13 个页面入口都可通过开发参数复现', async ({ page }) => {
  const surfaces = {
    home: '.playlist-row', library: '.list', playlist: '.list', albums: '.browse .grid', artists: '.browse .grid',
    album: '.list', artist: '.list', search: '.content-header', nowPlaying: '.np-info',
    settings: '.settings-page', beets: '.beets-page', live: '.live', broadcast: '.station-list',
  };
  for (const [route, selector] of Object.entries(surfaces)) {
    await page.goto(`/?backend=mock&host=web&page=${route}`);
    await expect(page.locator(`.main ${selector}`).first(), route).toBeVisible();
    if (route === 'search') {
      await page.getByRole('searchbox', { name: '搜索本地曲库', exact: true }).fill('晨雾');
      await page.getByRole('searchbox', { name: '搜索本地曲库', exact: true }).press('Enter');
      await expect(page.locator('.main .list')).toBeVisible();
    }
  }
});

test('Web 能力与频道工作台可通过源码预览直接操作', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=broadcast');
  await expect(page.getByRole('heading', { name: '推流', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /均衡器/ })).toHaveCount(0);
  const station = page.locator('[data-station="preview-night"]');
  await expect(station.getByRole('img', { name: '开播中' })).toBeVisible();
  await station.getByRole('button', { name: '配置', exact: true }).click();
  await expect(page.getByRole('heading', { name: '系统歌单', exact: false })).toBeVisible();
  await expect(page.locator('#program-__library__')).toBeVisible();
  await expect(page.locator('#program-pl-1 .week-grid')).toBeAttached();
  await page.locator('#program-__library__').getByRole('combobox', { name: '歌单内顺序', exact: true }).click();
  await page.getByRole('option', { name: '按歌单顺序', exact: true }).click();
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await expect(page.getByRole('button', { name: '应用更新', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '应用更新', exact: true }).click();
  await expect(page.locator('.program-status')).toHaveText('配置已应用');
  await page.getByRole('button', { name: '返回列表', exact: true }).click();
  await station.getByRole('button', { name: '停播', exact: true }).click();
  await expect(station.getByRole('img', { name: '已停播' })).toBeVisible();
  await station.getByRole('button', { name: '开播', exact: true }).click();
  await expect(station.getByRole('img', { name: '开播中' })).toBeVisible();
});

test('频道新建、编辑与删除只修改当前预览内存', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=broadcast');
  await page.getByRole('button', { name: '新建频道', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('textbox').first().fill('预览新频道');
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('heading', { name: /预览新频道/ })).toBeVisible();
  await page.getByRole('button', { name: '编辑频道信息', exact: true }).click();
  await dialog.getByRole('textbox').first().fill('已改名频道');
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('heading', { name: /已改名频道/ })).toBeVisible();
  await page.getByRole('button', { name: '删除', exact: true }).click();
  await page.getByRole('alertdialog').getByRole('button', { name: '删除', exact: true }).click();
  await expect(page.getByRole('button', { name: '新建频道', exact: true })).toBeVisible();
  await expect(page.getByText('已改名频道', { exact: false })).toHaveCount(0);
});

test('Web 目录输入、空态与缺失来源可复现', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=settings');
  await page.getByRole('button', { name: '添加目录', exact: true }).click();
  await expect(page.getByRole('dialog').getByRole('textbox')).toBeVisible();
  await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click();
  await page.goto('/?backend=mock&host=web&page=broadcast&scene=empty');
  await expect(page.locator('[data-station]')).toHaveCount(0);
  await expect(page.getByText('还没有频道。新建一个，编排歌单与节目表后开播。')).toBeVisible();
  await page.goto('/?backend=mock&host=web&page=broadcast&scene=issues');
  await page.locator('[data-station="preview-night"]').getByRole('button', { name: '配置', exact: true }).click();
  await expect(page.locator('.program-issues')).toContainText('deleted-playlist');
  await page.goto('/?backend=mock&host=web&page=broadcast&scene=runtime-error');
  await page.locator('[data-station="preview-night"]').getByRole('button', { name: '配置', exact: true }).click();
  await expect(page.locator('.program-error')).toContainText('Liquidsoap process unavailable');
});

test('封面与歌词来自本地 fixture，无歌词和长文本可切换', async ({ page }) => {
  await page.goto('/?backend=mock&page=nowPlaying&scene=long&theme=light&lang=en&scale=125');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await expect(page.locator('html')).toHaveAttribute('data-lang', 'en');
  await expect(page.locator('.np-info h1')).toContainText('A very long track title');
  await expect(page.locator('.np-cover')).toHaveJSProperty('naturalWidth', 512);
  await expect(page.locator('.lyrics-panel')).toContainText('这是开发预览歌词');
  await page.goto('/?backend=mock&page=nowPlaying&lyrics=none');
  await expect(page.locator('.lyrics-panel')).toContainText('没有歌词');
});

for (const gate of ['password', 'token']) {
  test(`登录 ${gate} 场景不发送认证请求或保存凭据`, async ({ page }) => {
    const requests: string[] = [];
    page.on('request', request => { if (request.url().includes('/auth/')) requests.push(request.url()); });
    await page.goto(`/?backend=mock&host=web&gate=${gate}&page=library`);
    await expect(page.locator('.gate')).toBeVisible();
    await page.locator('.gate input').fill('wrong');
    await page.locator('.gate button').click();
    await expect(page.getByRole('alert')).toBeVisible();
    await page.locator('.gate input').fill('preview');
    await page.locator('.gate button').click();
    await expect(page.getByRole('heading', { name: '全部曲目', exact: true })).toBeVisible();
    expect(requests).toEqual([]);
    expect(await page.evaluate(() => localStorage.getItem('aiya.authToken'))).toBeNull();
  });
}

test('HTTP 口令错误保留认证反馈', async ({ page }) => {
  await page.route('**/auth/state', route => route.fulfill({ json: { mode: 'password', required: true } }));
  await page.route('**/auth/login', route => route.fulfill({ status: 401, json: {} }));
  await page.goto('/?backend=http');
  await page.locator('.gate input').fill('wrong');
  await page.locator('.gate button').click();
  await expect(page.getByRole('alert')).toBeVisible();
});

for (const mode of ['password', 'token']) {
  test(`HTTP ${mode} 登录保存凭据并在重载后发送认证头`, async ({ page }) => {
    const authHeaders: (string | undefined)[] = [];
    await page.route('**/auth/state', route => {
      authHeaders.push(route.request().headers().authorization);
      return route.fulfill({ json: { mode, required: true } });
    });
    const passwords: unknown[] = [];
    await page.route('**/auth/login', route => {
      passwords.push(route.request().postDataJSON());
      return route.fulfill({ json: { token: 'preview-session' } });
    });
    await page.goto('/?backend=http');
    await expect(page.locator('.gate input')).toBeFocused();
    await page.locator('.gate input').fill(mode === 'token' ? '  preview-session  ' : 'preview-password');
    await Promise.all([page.waitForEvent('load'), page.locator('.gate button').click()]);
    await expect(page.locator('.gate input')).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem('aiya.authToken'))).toBe('preview-session');
    expect(authHeaders).toContain('Bearer preview-session');
    expect(passwords).toEqual(mode === 'password' ? [{ password: 'preview-password' }] : []);
  });
}
