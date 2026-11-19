import type { Page } from '@playwright/test';

type ContrastSample = { selector: string; color: number[]; opacity: number; rects: { x: number; y: number; width: number; height: number }[] };


/** 隐去字形但保留面、衬底与布局，从浏览器合成截图取背景，再计算文字有效颜色。 */
export async function measureTextContrast(page: Page, selectors: string[]) {
  const samples = await page.evaluate(selectors => {
    const samples: { selector: string; color: number[]; opacity: number; rects: { x: number; y: number; width: number; height: number }[] }[] = [];
    for (const selector of selectors) {
      for (const node of document.querySelectorAll<HTMLElement>(selector)) {
        const style = getComputedStyle(node);
        let opacity = 1;
        for (let parent: HTMLElement | null = node; parent; parent = parent.parentElement) opacity *= Number(getComputedStyle(parent).opacity);
        const range = document.createRange(), walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
        const boxes: DOMRect[] = [];
        for (let text = walker.nextNode(); text; text = walker.nextNode()) {
          if (!text.textContent?.trim()) continue;
          range.selectNodeContents(text); boxes.push(...range.getClientRects());
        }
        const clip = node.closest('.lyrics-body')?.getBoundingClientRect();
        let left = 0, top = 0, right = innerWidth, bottom = innerHeight;
        for (let parent: HTMLElement | null = node; parent; parent = parent.parentElement) {
          const css = getComputedStyle(parent), rect = parent.getBoundingClientRect();
          if (/(hidden|clip|auto|scroll)/.test(css.overflowX)) { left = Math.max(left, rect.left); right = Math.min(right, rect.right); }
          if (/(hidden|clip|auto|scroll)/.test(css.overflowY)) { top = Math.max(top, rect.top); bottom = Math.min(bottom, rect.bottom); }
        }
        const rects = boxes.map(r => ({ x: Math.max(left, r.left), y: Math.max(top, r.top),
          width: Math.min(right, r.right) - Math.max(left, r.left), height: Math.min(bottom, r.bottom) - Math.max(top, r.top) }))
          .filter(r => r.width > 1 && r.height > 1 && (!clip || r.y >= clip.top + clip.height * .1 && r.y + r.height <= clip.bottom - clip.height * .1)
            && node.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)));
        if (rects.length) samples.push({ selector, color: style.color.match(/[\d.]+/g)!.map(Number), opacity, rects });
        node.setAttribute('data-contrast-probe', '');
      }
    }
    return samples;
  }, selectors);
  const style = await page.addStyleTag({ content: '[data-contrast-probe], [data-contrast-probe] * { -webkit-text-fill-color:transparent !important; text-shadow:none !important; }' });
  const png = await page.screenshot();
  await style.evaluate(node => node.remove());
  await page.locator('[data-contrast-probe]').evaluateAll(nodes => nodes.forEach(node => node.removeAttribute('data-contrast-probe')));
  return readContrasts(page, png, samples);
}

async function readContrasts(page: Page, png: Buffer, samples: ContrastSample[]) {
  return page.evaluate(async ({ base64, samples }) => {
    const bytes = Uint8Array.from(atob(base64), char => char.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
    const canvas = document.createElement('canvas'); canvas.width = bitmap.width; canvas.height = bitmap.height;
    const context = canvas.getContext('2d')!; context.drawImage(bitmap, 0, 0); bitmap.close();
    const { data, width, height } = context.getImageData(0, 0, canvas.width, canvas.height);
    const luminance = (color: number[]) => color.slice(0, 3).map(v => { const s = v / 255; return s <= .04045 ? s / 12.92 : ((s + .055) / 1.055) ** 2.4; })
      .reduce((sum, value, i) => sum + value * [.2126, .7152, .0722][i], 0);
    return samples.map(sample => {
      let minimum = Infinity; let pixels = 0;
      const alpha = (sample.color[3] ?? 1) * sample.opacity;
      for (const rect of sample.rects) {
        for (let y = Math.ceil(rect.y * devicePixelRatio); y < Math.min(height, (rect.y + rect.height) * devicePixelRatio); y += 2) {
          for (let x = Math.ceil(rect.x * devicePixelRatio); x < Math.min(width, (rect.x + rect.width) * devicePixelRatio); x += 2) {
            const offset = (y * width + x) * 4, background = Array.from(data.slice(offset, offset + 3));
            const foreground = background.map((value, i) => sample.color[i] * alpha + value * (1 - alpha));
            const a = luminance(background), b = luminance(foreground);
            minimum = Math.min(minimum, (Math.max(a, b) + .05) / (Math.min(a, b) + .05)); pixels++;
          }
        }
      }
      return { selector: sample.selector, contrast: minimum, pixels, opacity: sample.opacity, color: sample.color };
    });
  }, { base64: png.toString('base64'), samples });
}

/** 从焦点环下方的实际合成背景计算非文本对比度。 */
export async function measureFocusContrast(page: Page, selector: string) {
  const target = page.locator(selector).first();
  await target.focus();
  const sample = await target.evaluate((node, selector) => {
    const element = node as HTMLElement, css = getComputedStyle(element), rect = element.getBoundingClientRect();
    const offset = Number.parseFloat(css.outlineOffset), thickness = Number.parseFloat(css.outlineWidth);
    element.setAttribute('data-focus-probe', '');
    return { selector, color: css.outlineColor.match(/[\d.]+/g)!.map(Number), opacity: 1,
      thickness, rects: [{x: rect.left + rect.width / 2 - 4, y: rect.top - offset - thickness, width: 8, height: thickness}] };
  }, selector);
  if (sample.thickness < 2) throw new Error(`Missing focus ring: ${selector}`);
  const style = await page.addStyleTag({ content: '[data-focus-probe] { outline-color:transparent !important; }' });
  const png = await page.screenshot();
  await style.evaluate(node => node.remove());
  await target.evaluate(node => node.removeAttribute('data-focus-probe'));
  return readContrasts(page, png, [sample]);
}
