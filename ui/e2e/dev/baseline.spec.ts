import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

const pages = ['library', 'settings', 'broadcast', 'nowPlaying'] as const;
const windows = [{ width: 960, height: 600 }, { width: 1280, height: 800 }, { width: 1440, height: 900 }];

for (const viewport of windows) for (const theme of ['dark', 'light']) for (const surface of pages) {
  test(`${surface} ${theme} ${viewport.width}×${viewport.height}`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.setViewportSize(viewport);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(`/?backend=mock&host=web&page=${surface}&theme=${theme}`);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.locator('.main')).toBeVisible();
    if (surface === 'library') await expect(page.locator('.main .row:not(.skeleton)').first()).toBeVisible();
    if (surface === 'settings') await expect(page.getByRole('heading', { name: '设置', exact: true })).toBeVisible();
    if (surface === 'broadcast') {
      await page.locator('[data-station="preview-night"]').getByRole('button', { name: '配置', exact: true }).click();
      await expect(page.locator('.program-queue')).toBeAttached();
    }
    if (surface === 'nowPlaying') {
      await expect(page.locator('.np-cover')).toHaveJSProperty('naturalWidth', 512);
      await expect(page.locator('.lyrics-panel')).toContainText('这是开发预览歌词');
    }
    await page.screenshot({ path: info.outputPath('page.png'), animations: 'disabled' });
    if (surface === 'broadcast') {
      await page.locator('#program-pl-1 .queue-controls').scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath('queue.png'), animations: 'disabled' });
      await page.locator('#program-pl-1 .week-grid').scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath('schedule.png'), animations: 'disabled' });
    }
    const metrics = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      const tokens = Object.fromEntries([
        '--control-h', '--row-h', '--titlebar-h', '--sidebar-w', '--player-h',
        '--space-1', '--space-2', '--space-3', '--space-4', '--space-5', '--space-6',
        '--radius-xs', '--radius-sm', '--radius-md', '--radius-lg', '--radius-xl',
        '--fs-base', '--fs-md', '--fs-display', '--text-2', '--surface-2', '--surface-4',
      ].map(name => [name, root.getPropertyValue(name).trim()]));
      const samples = ['.content-header h1', '.content-header p', '.list .row:not(.skeleton)', '.k-btn', '.k-field-box', '.k-select-trigger', '.workbench-heading h2', '.queue-controls', '.week-grid', '.hour', '.np-info h1', '.np-cover', '.lyric-line', '.setting-row'];
      return {
        viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
        rootFontSize: root.fontSize, tokens,
        samples: samples.flatMap(selector => {
          const node = document.querySelector(selector);
          if (!node) return [];
          const style = getComputedStyle(node);
          const rect = node.getBoundingClientRect();
          return [{ selector, fontSize: style.fontSize, lineHeight: style.lineHeight, borderRadius: style.borderRadius,
            padding: style.padding, gap: style.gap, opacity: style.opacity, color: style.color, background: style.backgroundColor,
            width: rect.width, height: rect.height, overflowX: style.overflowX }];
        }),
        horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
      };
    });
    await info.attach('metrics', { body: JSON.stringify(metrics, null, 2), contentType: 'application/json' });
    await writeFile(info.outputPath('metrics.json'), JSON.stringify(metrics, null, 2));
    expect(metrics.viewport).toMatchObject(viewport);
    expect(errors).toEqual([]);
  });
}
