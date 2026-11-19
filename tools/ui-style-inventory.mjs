import { readdir, readFile, mkdir, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const src = join(root, 'ui/src');
const output = join(root, 'ui/test-results/ui-style-inventory.json');
const properties = ['border-radius', 'font-size', 'padding', 'gap', 'opacity', 'background', 'box-shadow', 'backdrop-filter'];

async function files(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const groups = await Promise.all(entries.map(entry => entry.isDirectory() ? files(join(dir, entry.name)) : [join(dir, entry.name)]));
  return groups.flat().sort();
}

const rows = [];
const kit = [];
for (const path of await files(src)) {
  if (!/\.(css|svelte)$/.test(path)) continue;
  const text = await readFile(path, 'utf8');
  const name = relative(root, path).replaceAll('\\', '/');
  if (name.startsWith('ui/src/components/ui/') && path.endsWith('.svelte')) {
    kit.push({ file: name, bits: [...text.matchAll(/import\s+\{([^}]+)\}\s+from\s+['"]bits-ui['"]/g)].map(match => match[1].trim()) });
  }
  const blocks = path.endsWith('.css') ? [{ text, offset: 0 }] : [...text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(match => ({ text: match[1], offset: match.index + match[0].indexOf(match[1]) }));
  for (const block of blocks) {
    const css = block.text.replace(/\/\*[\s\S]*?\*\//g, comment => comment.replace(/[^\n]/g, ' '));
    const pattern = new RegExp(`(?<![\\w-])(${properties.join('|')})\\s*:\\s*([^;}]+)`, 'g');
    for (const match of css.matchAll(pattern)) {
      const value = match[2].trim();
      rows.push({ file: name, line: text.slice(0, block.offset + match.index).split('\n').length, property: match[1], value,
        token: value.includes('var(--'), literal: /(?:\d(?:px|rem)|rgba?\(|#[a-f\d]{3,8}\b|^0?\.\d+$)/i.test(value) });
    }
  }
}
const report = {
  note: '静态 CSS 声明盘点；字面量是复核线索，包含合理的几何与发丝线，不等于缺陷。行内 style 和动态值另由页面审查覆盖。',
  kit,
  summary: Object.fromEntries(properties.map(property => [property, {
    declarations: rows.filter(row => row.property === property).length,
    literals: rows.filter(row => row.property === property && row.literal).length,
  }])),
  declarations: rows,
};
await mkdir(join(root, 'ui/test-results'), { recursive: true });
await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
console.log(`UI kit: ${kit.length}; CSS declarations: ${rows.length}; report: ${relative(root, output)}`);
