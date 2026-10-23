// 生成 1024x1024 占位应用图标（深色圆角方块 + 播放三角）。
// 仅依赖 node:zlib 手工编码 PNG；被 `tauri icon` 作为源图使用。
// TODO(P3 美化)：替换为正式设计稿后重跑 `npm run icon`。
import { deflateSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const SIZE = 1024;
const OUT = resolve(fileURLToPath(new URL('.', import.meta.url)), 'icon-source.png');

/* ---------------- PNG 编码 ---------------- */

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePng(rgba, width, height) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([
    sig,
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/* ---------------- 绘制 ---------------- */

const px = Buffer.alloc(SIZE * SIZE * 4);
// 主题色：深底 + 靛紫渐变，与 UI 深色主题同族
const BG_TOP = [30, 30, 46];
const BG_BOTTOM = [49, 46, 82];
const ACCENT = [124, 108, 240];

const radius = 180;
const inset = 0;

function inRoundedSquare(x, y) {
  const r = radius;
  const minX = inset + r, maxX = SIZE - inset - r;
  const minY = inset + r, maxY = SIZE - inset - r;
  if (x < minX && y < minY) return (x - minX) ** 2 + (y - minY) ** 2 <= r * r;
  if (x > maxX && y < minY) return (x - maxX) ** 2 + (y - minY) ** 2 <= r * r;
  if (x < minX && y > maxY) return (x - minX) ** 2 + (y - maxY) ** 2 <= r * r;
  if (x > maxX && y > maxY) return (x - maxX) ** 2 + (y - maxY) ** 2 <= r * r;
  return x >= inset && x < SIZE - inset && y >= inset && y < SIZE - inset;
}

// 播放三角（居中偏右的视觉重心）
const cx = SIZE / 2;
const cy = SIZE / 2;
const tri = [
  [cx - 140, cy - 200],
  [cx - 140, cy + 200],
  [cx + 220, cy],
];

function sign(p1, p2, p3) {
  return (p1[0] - p3[0]) * (p2[1] - p3[1]) - (p2[0] - p3[0]) * (p1[1] - p3[1]);
}

function inTriangle(x, y) {
  const [a, b, c] = tri;
  const d1 = sign([x, y], a, b);
  const d2 = sign([x, y], b, c);
  const d3 = sign([x, y], c, a);
  const neg = d1 < 0 || d2 < 0 || d3 < 0;
  const pos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(neg && pos);
}

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const i = (y * SIZE + x) * 4;
    if (!inRoundedSquare(x, y)) {
      px[i + 3] = 0; // 透明
      continue;
    }
    const t = y / SIZE;
    let r = BG_TOP[0] + (BG_BOTTOM[0] - BG_TOP[0]) * t;
    let g = BG_TOP[1] + (BG_BOTTOM[1] - BG_TOP[1]) * t;
    let b = BG_TOP[2] + (BG_BOTTOM[2] - BG_TOP[2]) * t;
    if (inTriangle(x, y)) {
      r = ACCENT[0];
      g = ACCENT[1];
      b = ACCENT[2];
    }
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = 255;
  }
}

writeFileSync(OUT, encodePng(px, SIZE, SIZE));
console.log(`written: ${OUT}`);
