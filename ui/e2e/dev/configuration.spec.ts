import { expect, test, type Page } from '@playwright/test';

const errors = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  const messages: string[] = [];
  errors.set(page, messages);
  page.on('pageerror', e => messages.push(e.message));
  page.on('console', m => { if (m.type() === 'error') messages.push(m.text()); });
});
test.afterEach(({ page }) => expect(errors.get(page)).toEqual([]));

test('快捷键名称与冲突双语翻译，录制可取消、清空和恢复默认', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=settings&lang=en');
  await page.getByRole('button', { name: 'Shortcuts', exact: true }).click();
  const play = page.getByRole('button', { name: 'Record shortcut for Play / Pause', exact: true });
  const next = page.getByRole('button', { name: 'Record shortcut for Next track', exact: true });
  await expect(page.locator('.shortcut-list')).not.toContainText('shell.keys.');
  await play.click();
  await page.keyboard.press('Escape');
  await expect(play).toHaveText('Space');
  await play.click();
  await page.keyboard.press('Backspace');
  await expect(play).toHaveText('Unset');
  await next.click();
  await page.keyboard.press('m');
  await expect(page.locator('.notice.danger')).toContainText('Next track');
  await expect(page.locator('.notice.danger')).toContainText('Toggle mute');
  await expect(page.locator('.notice.danger')).not.toContainText('shell.keys.');
  await page.getByRole('button', { name: 'Reset to defaults', exact: true }).click();
  await expect(play).toHaveText('Space');
  await expect(page.locator('.notice.danger')).toHaveCount(0);
});

test('频谱选择遵循父开关且保留选值，山形参数只在山形显示', async ({ page }) => {
  await page.goto('/?backend=mock&page=settings');
  const bg = page.getByRole('combobox', { name: '背景频谱形态', exact: true });
  const fg = page.getByRole('combobox', { name: '频谱形态', exact: true });
  const glow = page.getByRole('radiogroup', { name: '背景频谱光效', exact: true });
  await bg.click();
  await page.getByRole('option', { name: '极光绸幕', exact: true }).click();
  await expect(glow).toHaveCount(0);
  await page.getByRole('switch', { name: '背景动态频谱', exact: true }).click();
  await expect(bg).toBeDisabled();
  await page.getByRole('switch', { name: '背景动态频谱', exact: true }).click();
  await expect(bg).toHaveText('极光绸幕');
  await bg.click();
  await page.getByRole('option', { name: '山形', exact: true }).click();
  await expect(glow.getByRole('radio').first()).toBeEnabled();
  await page.getByRole('switch', { name: '正在播放页频谱', exact: true }).click();
  await expect(fg).toBeDisabled();
  await page.getByRole('switch', { name: '全局 3D 效果', exact: true }).click();
  await expect(page.getByRole('switch', { name: '背景动态频谱', exact: true })).toBeDisabled();
  await expect(bg).toBeDisabled();
});

test('Web 设置隐藏桌面动作，目录验证仍在同一模态框内', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=settings');
  await expect(page.getByRole('button', { name: '打开目录', exact: true })).toHaveCount(0);
  await expect(page.getByRole('radiogroup', { name: '音频输出', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '添加目录', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('textbox').fill('relative/path');
  await dialog.getByRole('textbox').press('Enter');
  await expect(dialog.getByRole('textbox')).toHaveAttribute('aria-invalid', 'true');
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  await expect(page.locator('.root-list')).not.toContainText('relative/path');
});

test('频道表单回车提交，删除确认初始取消、遮罩不关闭、Esc 回焦', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=broadcast');
  await page.getByRole('button', { name: '新建频道', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('textbox', { name: /频道名/ }).fill('A3 keyboard radio');
  await dialog.getByRole('textbox', { name: /频道名/ }).press('Enter');
  await expect(page.locator('.workbench-heading')).toContainText('A3 keyboard radio');
  const remove = page.getByRole('button', { name: '删除', exact: true });
  await remove.click();
  const alert = page.getByRole('alertdialog');
  await expect(alert.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await page.locator('.k-dlg-overlay').click({ position: { x: 4, y: 4 } });
  await expect(alert).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(alert).toHaveCount(0);
  await expect(remove).toBeFocused();
});

test('周视图连续拖选、间隙命中、键盘与 24:00 输入同步且一次保存', async ({ page }) => {
  await page.goto('/?backend=mock&host=web&page=broadcast');
  await page.locator('[data-station="preview-night"]').getByRole('button', { name: '配置', exact: true }).click();
  const queue = page.locator('#program-pl-1');
  const grid = queue.locator('.week-grid');
  await grid.scrollIntoViewIfNeeded();
  const slots = queue.locator('.slot-row');
  const before = await slots.count();
  for (const [from, to] of [[0, 2], [50, 52]]) {
    const a = await grid.locator(`[data-index="${from}"]`).boundingBox();
    const b = await grid.locator(`[data-index="${to}"]`).boundingBox();
    await page.mouse.move(a!.x + 1, a!.y + 1);
    await page.mouse.down();
    await page.mouse.move(b!.x + 1, b!.y + 1, { steps: 5 });
    await expect(page.locator('html')).toHaveClass(/station-selecting/);
    await page.mouse.up();
  }
  await expect(slots).toHaveCount(before + 2);
  await grid.locator('[data-index="0"]').focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Enter');
  await expect(slots).toHaveCount(before + 3);
  const end = slots.last().getByRole('textbox', { name: '结束', exact: true });
  await end.fill('24:00');
  await expect(end).toHaveValue('24:00');
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await expect(page.getByRole('button', { name: '应用更新', exact: true })).toBeVisible();
  await expect(slots).toHaveCount(before + 3);
});

test('Beets 插件使用整行标签和 Bits 勾选，删除组件使用确认弹窗', async ({ page }) => {
  await page.goto('/?backend=mock&page=beets');
  await page.getByRole('tab', { name: '安装', exact: true }).click();
  const checks = page.locator('.pick').getByRole('checkbox');
  await expect(checks).toHaveCount(4);
  const editable = page.locator('.pick label').filter({ has: page.locator('button:not(:disabled)') }).first();
  const box = editable.getByRole('checkbox');
  const before = await box.getAttribute('aria-checked');
  await editable.locator('.pick-title').click();
  await expect(box).toHaveAttribute('aria-checked', before === 'true' ? 'false' : 'true');
  await page.getByRole('button', { name: '删除组件', exact: true }).click();
  const alert = page.getByRole('alertdialog');
  await expect(alert.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: '删除组件', exact: true })).toBeFocused();
});

test('歌单慢保存只提交一次，忙碌时不能关闭或修改，完成后关闭', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    const create = ipc.playlist.create;
    Reflect.set(window, 'a3PlaylistCalls', 0);
    ipc.playlist.create = async (...args: unknown[]) => {
      Reflect.set(window, 'a3PlaylistCalls', Reflect.get(window, 'a3PlaylistCalls') + 1);
      await new Promise(resolve => window.addEventListener('a3-save-release', resolve, { once: true }));
      return create(...args);
    };
  });
  await page.getByRole('button', { name: '新建歌单', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const name = dialog.getByRole('textbox', { name: '名称', exact: true });
  await name.fill('A3 slow save');
  await name.press('Enter');
  await expect(name).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '创建', exact: true })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '取消', exact: true })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '关闭', exact: true })).toBeDisabled();
  await page.keyboard.press('Escape');
  await page.locator('.k-dlg-overlay').click({ position: { x: 4, y: 4 } });
  await expect(dialog).toBeVisible();
  await dialog.locator('form').dispatchEvent('submit');
  expect(await page.evaluate(() => Reflect.get(window, 'a3PlaylistCalls'))).toBe(1);
  await page.evaluate(() => window.dispatchEvent(new Event('a3-save-release')));
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.sidebar .playlist-item').filter({ hasText: 'A3 slow save' })).toHaveCount(1);
});

test('歌单创建失败保留名称和规则，取消不再写入', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    Reflect.set(window, 'a3PlaylistCalls', 0);
    ipc.playlist.create = async () => {
      Reflect.set(window, 'a3PlaylistCalls', Reflect.get(window, 'a3PlaylistCalls') + 1);
      throw { code: 'io', message: 'A3 save failed', retryable: true };
    };
  });
  await page.getByRole('button', { name: '新建歌单', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const name = dialog.getByRole('textbox', { name: '名称', exact: true });
  await name.fill('A3 smart draft');
  await dialog.getByRole('switch', { name: '动态歌单', exact: true }).click();
  await dialog.getByRole('button', { name: /添加条件/ }).click();
  await dialog.getByRole('combobox').first().click();
  await page.getByRole('option', { name: '艺人名', exact: true }).click();
  const artist = dialog.getByRole('textbox', { name: '艺人名', exact: true });
  await artist.fill('draft artist');
  await dialog.getByRole('button', { name: '创建', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText('A3 save failed');
  await expect(name).toHaveValue('A3 smart draft');
  await expect(artist).toHaveValue('draft artist');
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(await page.evaluate(() => Reflect.get(window, 'a3PlaylistCalls'))).toBe(1);
  await expect(page.locator('.sidebar .playlist-item').filter({ hasText: 'A3 smart draft' })).toHaveCount(0);
});

test('动态歌单改名后规则保存失败保留草稿，可重试完成', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.evaluate(async () => {
    const clientPath = '/src/lib/ipc/client.ts';
    const overlayPath = '/src/stores/overlay.svelte.ts';
    const { ipc } = await import(clientPath);
    const { overlay } = await import(overlayPath);
    const { id } = await ipc.playlist.create('A3 existing smart', undefined, { all: [{ field: 'artist', op: 'contains', value: '周' }] });
    const smart = (await ipc.playlist.list()).find((p: { id: string }) => p.id === id);
    const rename = ipc.playlist.rename, setRule = ipc.playlist.setRule;
    const calls: string[] = [];
    Reflect.set(window, 'a3PlaylistCalls', calls);
    ipc.playlist.rename = async (...args: unknown[]) => { calls.push('name'); return rename(...args); };
    ipc.playlist.setRule = async (...args: unknown[]) => {
      calls.push('rule');
      if (calls.filter(c => c === 'rule').length === 1) throw { code: 'io', message: 'A3 rule failed', retryable: true };
      return setRule(...args);
    };
    overlay.dialog = { kind: 'playlistEdit', playlist: smart };
  });
  const dialog = page.getByRole('dialog');
  const name = dialog.getByRole('textbox', { name: '名称', exact: true });
  await name.fill('A3 renamed smart');
  const conditions = await dialog.locator('.rule-row').count();
  await dialog.getByRole('button', { name: /添加条件/ }).click();
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText('A3 rule failed');
  await expect(name).toHaveValue('A3 renamed smart');
  await expect(dialog.locator('.rule-row')).toHaveCount(conditions + 1);
  expect(await page.evaluate(() => Reflect.get(window, 'a3PlaylistCalls'))).toEqual(['name', 'rule']);
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.sidebar .playlist-item').filter({ hasText: 'A3 renamed smart' })).toHaveCount(1);
});

for (const theme of ['dark', 'light']) for (const scale of [125, 150]) {
  test(`配置页面 ${theme} ${scale}% 在 960×600 可访问全部控件`, async ({ page }, info) => {
    await page.setViewportSize({ width: 960, height: 600 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    const lang = theme === 'dark' ? 'zh' : 'en';
    const prefix = `/?backend=mock&host=web&theme=${theme}&lang=${lang}&scale=${scale}&scene=long`;
    for (const route of ['settings', 'beets', 'live', 'broadcast']) {
      await page.goto(`${prefix}&page=${route}`);
      await expect(page.locator('.main')).toBeVisible();
      if (route === 'settings') {
        await expect(page.locator('.settings-scroll')).toHaveCSS('padding-left', `${22 * scale / 100}px`);
        await page.getByRole('button', { name: lang === 'zh' ? '外观' : 'Appearance', exact: true }).click();
        const overflow = await page.locator('.setting-row').evaluateAll(nodes => nodes.filter(n => n.scrollWidth > n.clientWidth + 2).map(n => n.textContent));
        expect(overflow).toEqual([]);
      }
      if (route === 'beets') {
        await page.getByRole('tab', { name: lang === 'zh' ? '安装' : 'Install', exact: true }).click();
        await expect(page.locator('.pick').getByRole('checkbox')).toHaveCount(4);
        await page.screenshot({ path: info.outputPath('beets-install.png') });
        await page.getByRole('tab', { name: lang === 'zh' ? '分析' : 'Analyze', exact: true }).click();
        await page.getByRole('button', { name: /添加条件|Add condition/ }).click();
        const clipped = await page.locator('.beets-panel').evaluateAll(nodes => nodes.filter(n => n.scrollWidth > n.clientWidth + 2).map(n => n.className));
        expect(clipped).toEqual([]);
        await page.screenshot({ path: info.outputPath('beets-analyze.png') });
        await page.getByRole('tab', { name: lang === 'zh' ? '整理' : 'Organize', exact: true }).click();
        expect(await page.locator('.beets-panel').evaluateAll(nodes => nodes.filter(n => n.scrollWidth > n.clientWidth + 2).map(n => n.className))).toEqual([]);
      }
      if (route === 'live') {
        await expect(page.locator('.live')).toHaveCSS('padding-left', `${22 * scale / 100}px`);
        const next = page.locator('.arrow-right button');
        await next.scrollIntoViewIfNeeded();
        const rect = await next.boundingBox();
        expect(rect!.x).toBeGreaterThanOrEqual(0);
        expect(rect!.x + rect!.width).toBeLessThanOrEqual(960);
        const card = page.locator('.live-card.pos-current .card-face');
        await card.scrollIntoViewIfNeeded();
        const box = await card.boundingBox();
        const main = await page.locator('.main').boundingBox();
        expect(box!.height).toBeLessThanOrEqual(main!.height);
        expect(box!.x).toBeGreaterThanOrEqual(main!.x);
        expect(box!.x + box!.width).toBeLessThanOrEqual(960);
      }
      if (route === 'broadcast') {
        await page.locator('[data-station="preview-night"]').getByRole('button', { name: lang === 'zh' ? '配置' : 'Configure', exact: true }).click();
        await page.locator('.week-picker').first().scrollIntoViewIfNeeded();
        await expect(page.locator('.hour').first()).toHaveCSS('font-size', `${11.5 * scale / 100}px`);
        const overflow = await page.locator('.broadcast, .program-queue, .k-card').evaluateAll(nodes => nodes.filter(n => n.scrollWidth > n.clientWidth + 2).map(n => n.className));
        expect(overflow).toEqual([]);
      }
      await page.screenshot({ path: info.outputPath(`${route}.png`), animations: 'disabled' });
    }
    for (const gate of ['password', 'token']) {
      await page.goto(`${prefix}&page=library&gate=${gate}`);
      await expect(page.locator('.gate input')).toBeVisible();
      await page.screenshot({ path: info.outputPath(`login-${gate}.png`) });
    }
  });
}
