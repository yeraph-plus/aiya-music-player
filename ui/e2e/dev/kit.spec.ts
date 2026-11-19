import { expect, test, type Page } from '@playwright/test';

const errors = new WeakMap<Page, string[]>();

async function expectPlayerControlsFit(page: Page) {
  const bounds = await page.locator('footer.player button').evaluateAll(nodes => nodes.map(node => {
    const r = node.getBoundingClientRect();
    return {label:node.getAttribute('aria-label'),left:r.left,right:r.right,top:r.top,bottom:r.bottom};
  }));
  expect(bounds.filter(r => r.left < 0 || r.right > 961)).toEqual([]);
  const overlaps = bounds.flatMap((a, i) => bounds.slice(i + 1).filter(b =>
    Math.min(a.right,b.right) - Math.max(a.left,b.left) > 1 &&
    Math.min(a.bottom,b.bottom) - Math.max(a.top,b.top) > 1,
  ).map(b => [a.label,b.label]));
  expect(overlaps).toEqual([]);
}
test.beforeEach(({ page }) => {
  const messages: string[] = [];
  errors.set(page, messages);
  page.on('pageerror', error => messages.push(error.message));
  page.on('console', message => {if (message.type() === 'error') messages.push(message.text());});
});
test.afterEach(({ page }) => expect(errors.get(page)).toEqual([]));

test('Field 保留输入标签、原生属性与 IME 回车边界', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await page.getByRole('button', { name: '新建歌单', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const input = dialog.getByRole('textbox').first();
  await expect(input).toBeFocused();
  await input.fill('组合输入边界');
  await input.dispatchEvent('keydown', {key:'Enter', isComposing:true, bubbles:true});
  await expect(dialog).toBeVisible();
  await expect(page.locator('.sidebar .playlist-item').filter({hasText:'组合输入边界'})).toHaveCount(0);
  await input.press('Enter');
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.sidebar .playlist-item').filter({hasText:'组合输入边界'})).toHaveCount(1);
  await expect(page.getByRole('button', {name:'新建歌单', exact:true})).toBeFocused();
  const search = page.getByRole('searchbox', {name:'搜索本地曲库', exact:true});
  await search.fill('晨雾');
  const clear = page.getByRole('banner').getByRole('button', {name:'清除搜索', exact:true});
  await expect(clear).toBeVisible();
  await clear.click();
  await expect(search).toHaveValue('');
  await expect(search).toBeFocused();
});

test('登录 Field 可访问，原生表单回车只提交一次', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=library&gate=password');
  const input = page.getByLabel('管理员口令', {exact:true});
  await expect(input).toHaveAttribute('name', 'password');
  await expect(input).toHaveAttribute('autocomplete', 'current-password');
  await input.fill('wrong');
  await input.press('Enter');
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(input).toHaveAttribute('aria-invalid', 'true');
  await expect(input).toHaveAttribute('aria-describedby', 'gate-error');
  await expect(input).toBeFocused();
  await input.fill('preview');
  await input.press('Enter');
  await expect(page.getByRole('heading', {name:'全部曲目',exact:true})).toBeVisible();
});

test('Beets Tabs 关联面板并支持方向键，设置锚点保留导航语义', async ({ page }) => {
  await page.goto('/?backend=mock&page=beets');
  const analyze = page.getByRole('tab', {name:'分析', exact:true});
  await expect(analyze).toHaveAttribute('aria-selected', 'true');
  const id = await analyze.getAttribute('aria-controls');
  await expect(page.getByRole('tabpanel', {name:'分析', exact:true})).toHaveAttribute('id', id!);
  await analyze.focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', {name:'整理', exact:true})).toBeFocused();
  await expect(page.getByRole('tabpanel', {name:'整理', exact:true})).toBeVisible();
  await page.goto('/?backend=mock&page=beets&beets=absent');
  await expect(page.getByRole('tab', {name:'分析', exact:true})).toBeDisabled();
  await expect(page.getByRole('tab', {name:'整理', exact:true})).toBeDisabled();
  await page.goto('/?backend=mock&page=settings');
  await expect(page.getByRole('tablist')).toHaveCount(0);
  await expect(page.locator('.settings-page nav')).toBeVisible();
});

test('非模态抽屉保留底栏控制，Tooltip 不截断 Esc', async ({ page }) => {
  await page.goto('/?backend=mock&page=nowPlaying');
  const trigger = page.locator('footer.player').getByRole('button', {name:/播放队列/});
  await trigger.click();
  await expect(page.locator('.queue-float')).toBeVisible();
  await page.locator('footer.player').getByRole('button', {name:'播放',exact:true}).click();
  await expect(page.locator('.queue-float')).toBeVisible();
  await page.locator('footer.player').getByRole('button', {name:'暂停',exact:true}).click();
  await expect(page.locator('.queue-float')).toBeVisible();
  await page.locator('.queue-float').getByRole('button', {name:'关闭队列',exact:true}).focus();
  await page.keyboard.press('Escape');
  await expect(page.locator('.queue-float')).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await expect(trigger).not.toHaveAttribute('title');
});

test('抽屉内菜单先关闭，第二次 Esc 关闭抽屉并回焦', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  const trigger = page.locator('footer.player').getByRole('button', {name:/均衡器/});
  await trigger.click();
  await page.locator('.eq-panel').getByRole('combobox').click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await expect(page.locator('.eq-panel')).toBeVisible();
  await expect(page.locator('.eq-panel').getByRole('combobox')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.locator('.eq-panel')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

for (const theme of ['dark', 'light']) for (const scale of [90, 110, 125, 150]) {
  test(`960 窗口 ${theme} ${scale}% 下频道弹窗与嵌套 Select 可操作`, async ({ page }, info) => {
    await page.setViewportSize({width:960,height:600});
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.goto(`/?backend=mock&host=web&page=broadcast&theme=${theme}&lang=en&scale=${scale}`);
    const create = page.getByRole('button', {name:'New station',exact:true});
    await create.click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    const visibility = dialog.getByRole('combobox');
    await visibility.click();
    await expect(page.getByRole('listbox')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('listbox')).toHaveCount(0);
    await expect(dialog).toBeVisible();
    await expect(visibility).toBeFocused();
    await expectPlayerControlsFit(page);
    await dialog.getByRole('textbox').last().scrollIntoViewIfNeeded();
    const rect = await dialog.boundingBox();
    expect(rect!.x).toBeGreaterThan(0);
    expect(rect!.y).toBeGreaterThan(0);
    expect(rect!.y + rect!.height).toBeLessThan(600);
    await page.screenshot({path:info.outputPath('channel-dialog.png'),animations:'disabled'});
    await dialog.getByRole('button', {name:'Cancel',exact:true}).click();
    await expect(dialog).toHaveCount(0);
    await expect(create).toBeFocused();
  });
}

for (const scale of [90, 110, 125, 150]) {
  test(`桌面能力 ${scale}% 下底栏操作不重叠，均衡器入口可用`, async ({page}) => {
    await page.setViewportSize({width:960,height:600});
    await page.goto(`/?backend=mock&page=nowPlaying&scale=${scale}`);
    await expectPlayerControlsFit(page);
    await page.locator('footer.player').getByRole('button', {name:/均衡器/}).click();
    await expect(page.locator('.eq-panel')).toBeVisible();
    await expectPlayerControlsFit(page);
  });
}
