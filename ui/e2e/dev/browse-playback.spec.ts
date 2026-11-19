import { expect, test, type Page } from '@playwright/test';

const errors = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  const messages: string[] = [];
  errors.set(page, messages);
  page.on('pageerror', e => messages.push(e.message));
  page.on('console', m => { if (m.type() === 'error') messages.push(m.text()); });
});
test.afterEach(({ page }) => { expect(errors.get(page)).toEqual([]); });

for (const route of ['album', 'artist', 'playlist', 'library']) {
  test(`${route} 只有一个播放全部入口，批量选择可用`, async ({ page }) => {
    await page.goto(`/?backend=mock&page=${route}`);
    await expect(page.locator('.list .row:not(.skeleton)').first()).toBeVisible();
    await expect(page.getByRole('button', { name: '播放全部', exact: true })).toHaveCount(1);
    const toolbar = page.locator('.list-toolbar');
    await expect(toolbar.getByRole('button', { name: '播放全部', exact: true })).toHaveCount(route === 'library' ? 1 : 0);
    await toolbar.getByRole('button', { name: '批量操作', exact: true }).click();
    await page.locator('.list .row').first().getByRole('checkbox').click();
    await expect(toolbar).toContainText('已选 1 首');
  });
}

test('队列移除 Enter/Space 不播放，Delete 和键盘移动保持焦点', async ({ page }) => {
  await page.goto('/?backend=mock&page=nowPlaying');
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  const rows = page.locator('.queue-row');
  await expect(rows).toHaveCount(8);
  await expect(rows.first().locator('button button')).toHaveCount(0);
  for (const key of ['Enter', 'Space']) {
    await rows.nth(1).getByRole('button', { name: '从队列移除', exact: true }).focus();
    await page.keyboard.press(key);
    await expect(rows).toHaveCount(key === 'Enter' ? 7 : 6);
    await expect(page.locator('footer').getByRole('button', { name: '播放', exact: true })).toBeVisible();
    await expect(rows.nth(1).locator('.q-play')).toBeFocused();
  }
  const title = await rows.first().locator('.q-title').textContent();
  await rows.first().locator('.q-play').focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect(rows.nth(1).locator('.q-title')).toHaveText(title!);
  await expect(rows.nth(1).locator('.q-play')).toBeFocused();
  await page.keyboard.press('Delete');
  await expect(rows).toHaveCount(5);
  await expect(rows.nth(1).locator('.q-play')).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator('footer').getByRole('button', { name: '暂停', exact: true })).toBeVisible();
});

test('队列把手拖排只改顺序，不触发播放', async ({ page }) => {
  await page.goto('/?backend=mock&page=nowPlaying');
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  const rows = page.locator('.queue-row');
  await expect(rows).toHaveCount(8);
  const title = await rows.first().locator('.q-title').textContent();
  const grip = await rows.first().locator('.q-grip').boundingBox();
  expect(grip).not.toBeNull();
  await page.mouse.move(grip!.x + grip!.width / 2, grip!.y + grip!.height / 2);
  await page.mouse.down();
  await page.mouse.move(grip!.x + grip!.width / 2, grip!.y + grip!.height / 2 + 92, { steps: 8 });
  await expect(page.locator('.queue-list .drop-line')).toBeVisible();
  await page.mouse.up();
  await expect(rows.nth(2).locator('.q-title')).toHaveText(title!);
  await expect(page.locator('footer').getByRole('button', { name: '播放', exact: true })).toBeVisible();
  await rows.first().locator('.q-play').press('Enter');
  await expect(page.locator('footer').getByRole('button', { name: '暂停', exact: true })).toBeVisible();
});

test('歌单键盘重排、虚拟窗口与详情返回位置保留', async ({ page }) => {
  await page.goto('/?backend=mock&page=playlist');
  const rows = page.locator('.list .row:not(.skeleton):not(.failed)');
  await expect(rows.first()).toBeVisible();
  const title = await rows.first().locator('.title').textContent();
  await rows.first().focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect(rows.nth(1).locator('.title')).toHaveText(title!);
  await expect(rows.nth(1)).toBeFocused();
  await page.locator('.list').evaluate(node => { node.scrollTop = 650; node.dispatchEvent(new Event('scroll')); });
  const top = await page.locator('.list').evaluate(node => node.scrollTop);
  await rows.last().locator('.link-cell').last().click();
  await expect(page.locator('.ch-kicker')).toHaveText('ALBUM');
  await page.getByRole('button', { name: '返回上一页', exact: true }).click();
  await expect(page.locator('.ch-kicker')).toHaveText('PLAYLIST');
  await expect.poll(() => page.locator('.list').evaluate(node => node.scrollTop)).toBeCloseTo(top, 0);
  expect(await rows.count()).toBeLessThan(100);
});

test('大队列保留虚拟化和重复曲目，移除末项后焦点仍有去处', async ({ page }) => {
  await page.goto('/?backend=mock&page=nowPlaying');
  await expect(page.locator('.np-cover')).toHaveJSProperty('naturalWidth', 512);
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    const queue = await ipc.player.queue();
    await ipc.player.playTracks(Array.from({ length: 1000 }, (_, i) => queue.ids[i % queue.ids.length]), 0);
    await ipc.player.toggle();
  });
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  const rows = page.locator('.queue-row');
  await expect(rows.first()).toBeVisible();
  expect(await rows.count()).toBeLessThan(40);
  await page.locator('.queue-list').evaluate(node => { node.scrollTop = node.scrollHeight; node.dispatchEvent(new Event('scroll')); });
  await expect(rows.last().locator('.q-play')).toHaveAttribute('data-queue-index', '999');
  await rows.last().getByRole('button', { name: '从队列移除', exact: true }).click();
  await expect(page.locator('[data-queue-index="998"]')).toBeFocused();
  await page.locator('.queue-panel').getByRole('button', { name: '清空', exact: true }).click();
  await expect(rows).toHaveCount(0);
  await page.getByRole('button', { name: '关闭队列', exact: true }).click();
  await page.locator('footer').getByRole('button', { name: /播放队列/ }).click();
  await expect(page.getByRole('button', { name: '关闭队列', exact: true })).toBeFocused();
});

test('曲库滚动恢复不被焦点跟随覆盖', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.locator('.list').evaluate(node => { node.scrollTop = 1300; node.dispatchEvent(new Event('scroll')); });
  await expect(page.locator('.list .row').first()).not.toHaveAttribute('data-index', '0');
  const top = await page.locator('.list').evaluate(node => node.scrollTop);
  const rows = page.locator('.list .row:not(.skeleton):not(.failed)');
  expect(await rows.count()).toBeLessThan(60);
  await rows.nth(7).locator('.link-cell').last().click();
  await expect(page.locator('.ch-kicker')).toHaveText('ALBUM');
  await page.getByRole('button', { name: '返回上一页', exact: true }).click();
  await expect(page.locator('.ch-kicker')).toHaveText('LIBRARY');
  await expect.poll(() => page.locator('.list').evaluate(node => node.scrollTop)).toBeCloseTo(top, 0);
});

test('浏览过滤与滚动位置随返回栈恢复', async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 600 });
  await page.goto('/?backend=mock&page=albums');
  await expect(page.locator('.browse .k-card').first()).toBeVisible();
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    const albums = ipc.library.albums;
    const base = (await albums()).items[0];
    ipc.library.albums = async () => ({ items: Array.from({ length: 130 }, (_, i) => ({ ...base, id: `browse-${i}`, title: `光 ${i}` })), total: 130 });
  });
  const filter = page.getByRole('searchbox', { name: '按专辑名过滤', exact: true });
  await filter.fill('光');
  const cards = page.locator('.browse .k-card');
  await expect(cards).toHaveCount(60);
  await page.getByRole('button', { name: '下一页', exact: true }).click();
  await expect(cards.first()).toContainText('光 60');
  await page.locator('.browse').evaluate(node => { node.scrollTop = 130; node.dispatchEvent(new Event('scroll')); });
  const top = await page.locator('.browse').evaluate(node => node.scrollTop);
  expect(top).toBeGreaterThan(0);
  await cards.nth(3).click();
  await expect(page.locator('.ch-kicker')).toHaveText('ALBUM');
  await page.getByRole('button', { name: '返回上一页', exact: true }).click();
  await expect(filter).toHaveValue('光');
  await expect(cards.first()).toContainText('光 60');
  await expect.poll(() => page.locator('.browse').evaluate(node => node.scrollTop)).toBeCloseTo(top, 0);
});

test('搜索换词立即隐藏旧结果，晚到响应不覆盖新词', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    const list = ipc.library.list;
    ipc.library.list = async (query: { search?: string }) => {
      const response = await list(query);
      await new Promise(r => setTimeout(r, query.search === '晨' ? 1000 : 30));
      return response;
    };
  });
  const search = page.getByRole('searchbox', { name: '搜索本地曲库', exact: true });
  await search.fill('晨');
  await expect(page.locator('.tracks-loading')).toBeVisible();
  await page.waitForTimeout(220);
  await search.fill('星');
  await expect(page.locator('.tracks-loading')).toBeVisible();
  await expect(page.locator('.list .row').first()).toBeVisible();
  await expect(page.locator('.list .row .title').first()).toContainText('星');
  await page.waitForTimeout(1100);
  await expect(page.locator('.list .row .title').first()).toContainText('星');
  await expect(page.locator('.content-header h1')).toContainText('星');
  await expect(page.getByRole('button', { name: '播放全部', exact: true })).toHaveCount(1);
});

test('专辑与艺人并行加载状态独立；搜索失败可原地重试', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await expect(page.locator('.list .row').first()).toBeVisible();
  await page.evaluate(async () => {
    const path = '/src/lib/ipc/client.ts';
    const { ipc } = await import(path);
    const albums = ipc.library.albums, artists = ipc.library.artists, list = ipc.library.list;
    let fail = true;
    ipc.library.albums = async (query?: string) => { await new Promise(r => setTimeout(r, 100)); return albums(query); };
    ipc.library.artists = async (query?: string) => { await new Promise(r => setTimeout(r, 1300)); return artists(query); };
    ipc.library.list = async (query: unknown) => { if (fail) { fail = false; throw { code:'io', message:'Temporary search failure', retryable:true }; } return list(query); };
  });
  await page.getByRole('searchbox', { name: '搜索本地曲库', exact: true }).fill('周');
  const sections = page.locator('.facet-row');
  await expect(sections.first()).toHaveAttribute('aria-busy', 'false');
  await expect(sections.nth(1)).toHaveAttribute('aria-busy', 'true');
  await expect(page.getByText('列表加载失败', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.locator('.list .row').first()).toBeVisible();
  await expect(sections.nth(1)).toHaveAttribute('aria-busy', 'false');
});

test('宫格请求失败显示重试，不显示空库', async ({ page }) => {
  await page.goto('/?backend=mock&page=albums&failCmd=library_albums');
  await expect(page.getByRole('button', { name: '重试', exact: true })).toBeVisible();
  await expect(page.getByText('列表加载失败', { exact: true })).toBeVisible();
  await expect(page.locator('.browse .grid')).toHaveCount(0);
});

test('EQ 十三个共享滑杆支持步进、边界、禁用与自定义预设', async ({ page }) => {
  await page.goto('/?backend=mock&page=library');
  await page.locator('footer').getByRole('button', { name: /均衡器/ }).click();
  const panel = page.locator('.eq-panel');
  await expect(panel.getByRole('slider')).toHaveCount(13);
  const select = panel.getByRole('combobox');
  await select.click();
  await page.getByRole('option', { name: '重低音', exact: true }).click();
  const gain = panel.getByRole('slider', { name: '前置增益', exact: true });
  await gain.focus();
  await gain.press('Home');
  await expect(gain).toHaveAttribute('aria-valuenow', '-12');
  await gain.press('ArrowRight');
  await expect(gain).toHaveAttribute('aria-valuenow', '-11.5');
  await expect(gain).toHaveAttribute('aria-valuetext', '-11.5 dB');
  await expect(select).toContainText('重低音');
  await panel.getByRole('slider', { name: '频段 31 Hz 增益', exact: true }).press('ArrowUp');
  await expect(select).toContainText('自定义');
  await gain.press('End');
  await expect(gain).toHaveAttribute('aria-valuenow', '12');
  await panel.getByRole('switch').click();
  for (const slider of await panel.getByRole('slider').all()) await expect(slider).toHaveAttribute('aria-disabled', 'true');
});

for (const theme of ['dark', 'light']) for (const scale of [125, 150]) {
  test(`960×600 ${theme} ${scale}% 浏览和播放内容不剪裁`, async ({ page }, info) => {
    await page.setViewportSize({ width: 960, height: 600 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    for (const route of ['home', 'library', 'playlist', 'album', 'artist', 'search', 'nowPlaying']) {
      await page.goto(`/?backend=mock&page=${route}&theme=${theme}&lang=${theme === 'dark' ? 'zh' : 'en'}&scale=${scale}&scene=long`);
      if (route === 'search') {
        await page.getByRole('searchbox').first().fill('周');
        await expect(page.locator('.list .row').first()).toBeVisible();
      } else if (route !== 'home' && route !== 'nowPlaying') await expect(page.locator('.list .row').first()).toBeVisible();
      else if (route === 'home') await expect(page.locator('.home-panel .row').first()).toBeVisible();
      else await expect(page.locator('.np-cover')).toBeVisible();
      const clipped = await page.locator('.list .row:not(.skeleton), .home-panel .row, .list-toolbar, .np-info').evaluateAll(nodes => nodes.flatMap(node => {
        const rect = node.getBoundingClientRect();
        return node.scrollWidth > node.clientWidth + 2 || rect.right > innerWidth + 1 ? [node.className] : [];
      }));
      expect(clipped, route).toEqual([]);
      if (route === 'nowPlaying') {
        await page.locator('.np-meta .np-line').last().scrollIntoViewIfNeeded();
        const bottom = await page.locator('.np-meta .np-line').last().boundingBox();
        const footer = await page.locator('footer.player').boundingBox();
        expect(bottom!.y + bottom!.height).toBeLessThanOrEqual(footer!.y);
      }
      await page.screenshot({ path: info.outputPath(`${route}.png`), animations: 'disabled' });
    }
  });
}
