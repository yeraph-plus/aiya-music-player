/**
 * 截图亮度统计：只算背景区域（排除标题栏、播放条、面板），给"亮不亮"一个数字。
 *
 * 为什么不在页面里读 canvas：CSS 渐变不在任何 canvas 里，只读 GL 画布会漏掉
 * 占比最大的那一层；要判断"整屏亮度"只能对合成后的截图取样。
 *
 * 用法：node tools/bench/imgstat.mjs a.png [b.png ...]
 */
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { readFile } from 'node:fs/promises';

const require = createRequire(resolve('ui/package.json'));
const { chromium } = require('playwright');

const files = process.argv.slice(2);
const b = await chromium.launch({ channel: 'msedge', headless: true });
const page = await b.newPage();
const rows = [];
for (const f of files) {
  const b64 = (await readFile(f)).toString('base64');
  const r = await page.evaluate(async (b64) => {
    const img = await new Promise((res, rej) => {
      const i = new Image();
      i.onload = () => res(i);
      i.onerror = rej;
      i.src = 'data:image/png;base64,' + b64;
    });
    const cv = new OffscreenCanvas(img.width, img.height);
    const cx = cv.getContext('2d');
    cx.drawImage(img, 0, 0);
    const d = cx.getImageData(0, 0, img.width, img.height).data;
    // 只取中段：上下各让开 12%（标题栏/播放条），左右让开 5%
    const x0 = Math.floor(img.width * 0.05);
    const x1 = Math.floor(img.width * 0.95);
    const y0 = Math.floor(img.height * 0.12);
    const y1 = Math.floor(img.height * 0.88);
    let sum = 0;
    let n = 0;
    let maxL = 0;
    const hist = new Array(8).fill(0);
    for (let y = y0; y < y1; y += 1) {
      for (let x = x0; x < x1; x += 1) {
        const i = (y * img.width + x) * 4;
        // Rec.709 相对亮度（近似，未做线性化——这层只用来横向比较）
        const l = 0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2];
        sum += l;
        n += 1;
        if (l > maxL) maxL = l;
        hist[Math.min(7, Math.floor(l / 32))] += 1;
      }
    }
    return {
      w: img.width,
      h: img.height,
      mean: +(sum / n).toFixed(1),
      max: +maxL.toFixed(0),
      // 直方图：0-31, 32-63, ... 224-255 各占比
      hist: hist.map((v) => +(100 * v / n).toFixed(1)),
    };
  }, b64);
  rows.push({ file: f.replace(/^.*[\\/]/, ''), ...r });
}
console.table(rows);
await b.close();
