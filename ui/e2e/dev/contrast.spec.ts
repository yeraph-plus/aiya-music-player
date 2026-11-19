import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import { measureTextContrast, measureFocusContrast } from './contrast';

for (const theme of ['dark', 'light']) for (const ambient of ['aurora', 'ember', 'meadow', 'dusk', 'mono']) {
  test(`播放页 ${theme} ${ambient} 的十八组合截图与元信息对比度`, async ({ page }, info) => {
    test.setTimeout(90_000);
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.goto(`/?backend=mock&page=nowPlaying&bg=aurora&fg=shore&theme=${theme}&ambient=${ambient}`);
    await page.getByRole('button', { name: '播放', exact: true }).click();
    await page.evaluate(async () => {
      const path = '/src/stores/player.svelte.ts', { player } = await import(path), source = player.getSpectrumSource();
      const full = new Float32Array(48).fill(1);
      Object.defineProperty(source, 'live', { get: () => true }); source.frame = () => full;
    });
    const records: object[] = [], failures: object[] = [];
    for (const bg of ['ridge', 'aurora', 'curtain']) for (const fg of ['orbit', 'ribbon', 'lattice', 'contour', 'string', 'shore']) {
      await page.evaluate(async ({ bg, fg }) => {
        const path = '/src/stores/ui.svelte.ts', { ui } = await import(path);
        ui.settings = { ...ui.settings, bgSpectrumStyle: bg, spectrumStyle: fg };
      }, { bg, fg });
      await page.waitForTimeout(1000);
      await page.screenshot({ path: info.outputPath(`${bg}-${fg}.png`) });
      // 字形阴影不适用移除 text-shadow 后的矩形背景测量，歌词由组合截图检查。
      const result = await measureTextContrast(page, ['.np-info h1', '.np-line', '.audio-spec']);
      records.push({ bg, fg, samples: result });
      expect(result.length).toBeGreaterThanOrEqual(4);
      failures.push(...result.filter(sample => sample.contrast < 4.5).map(sample => ({ bg, fg, ...sample })));
    }
    await writeFile(info.outputPath('contrast.json'), JSON.stringify({ theme, ambient, records, failures }, null, 2));
    expect(errors).toEqual([]); expect(failures).toEqual([]);
  });
}

for (const theme of ['dark', 'light']) for (const ambient of ['aurora', 'ember', 'meadow', 'dusk', 'mono']) {
  test(`代表页 ${theme} ${ambient} 的正文对比度`, async ({ page }, info) => {
    const records: object[] = [], failures: object[] = [];
    await page.emulateMedia({ reducedMotion: 'reduce' });
    for (const surface of ['library', 'settings', 'broadcast']) {
      await page.goto(`/?backend=mock&host=web&page=${surface}&theme=${theme}&ambient=${ambient}`);
      await expect(page.locator('.content-header h1')).toBeVisible();
      if (surface === 'library') await expect(page.locator('.main .row:not(.skeleton)').first()).toBeVisible();
      if (surface === 'broadcast') {
        await page.locator('[data-station="preview-night"]').getByRole('button', { name: '配置', exact: true }).click();
        await expect(page.locator('.program-queue')).toBeAttached();
      }
      const samples = await measureTextContrast(page, ['.content-header h1', '.content-header p', '.content-header .ch-kicker', '.setting-title', '.setting-hint', '.section-hint', '.k-card-title', '.k-card-subtitle', '.k-select-trigger']);
      expect(samples.length).toBeGreaterThan(0);
      failures.push(...samples.filter(sample => sample.contrast < 4.5).map(sample => ({ surface, ...sample })));
      const focus = await measureFocusContrast(page, '.main button:not(:disabled)');
      failures.push(...focus.filter(sample => sample.contrast < 3).map(sample => ({ surface, focus: true, ...sample })));
      records.push({ surface, samples, focus });
    }
    await writeFile(info.outputPath('text-contrast.json'), JSON.stringify({ theme, ambient, records, failures }, null, 2));
    expect(failures).toEqual([]);
  });
}
