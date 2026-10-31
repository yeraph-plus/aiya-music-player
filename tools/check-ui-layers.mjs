#!/usr/bin/env node
// UI 门禁（两件事，历史上只有第一件）：
//
// 1. 分层：组件/视图不许绕过 lib 直接摸宿主。
//
// 为什么需要机器检查：`check-boundary.mjs` 守的是 Rust 侧的 beets 边界，UI
// 侧一直只有"约定"——而约定在第一次赶工时就会破。真实发生过的那次是
// `TitleBar.svelte` 直连 `@tauri-apps/api/window`：一段"就三行"的窗口控制，
// 顺着写下去就会长出第二处、第三处，然后窗口能力与数据协议各自长出一套
// 降级逻辑（浏览器预览下一半按钮能用一半炸）。
//
// 三条规则对应三层职责：
//   components/ + views/  只渲染、只读 store，不碰 IPC 也不碰宿主
//   stores/               编排 IPC 调用（经 lib/ipc/client），不碰宿主
//   lib/                  唯一能碰 `@tauri-apps/*` 的地方
//
// 豁免：`lib/ipc/transport.ts`（数据协议换气口）、`lib/window-chrome.ts`
// （窗口 chrome），以及 `main.ts` / `App.svelte` 这类入口——它们本来就是
// 组装的最后一步。豁免不是"白名单目录"，是"这三类文件确实在边界外"。
//
// 2. z-index 取值必须来自 token 梯（发布前 UI 审查 P1-3）。此前 14 个裸值
// 靠各文件注释维系，Toast/Menu/Select 同值 60、谁在上取决于 portal 的 DOM
// 顺序。阶梯契约见 tokens.css 的 --z-* 区段；这里只验"写的是 var(--z-*)"，
// 裸数字一律红——否则收编成果两周内回潮。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), '..');
const SRC = join(ROOT, 'ui/src');

/** @type {{id:string, dirs:string[], re:RegExp, why:string}[]} */
const RULES = [
  {
    id: 'host-api',
    dirs: ['components', 'views', 'stores'],
    re: /from\s+['"]@tauri-apps\//,
    why: '宿主 API 只许出现在 lib/（数据走 lib/ipc，窗口走 lib/window-chrome）',
  },
  {
    id: 'ipc-transport',
    dirs: ['components', 'views', 'stores'],
    re: /from\s+['"][^'"]*ipc\/transport['"]/,
    why: 'transport 是换气口实现，只由 lib/ipc/{client,events} 使用',
  },
  {
    id: 'ipc-direct',
    dirs: ['components', 'views'],
    re: /from\s+['"][^'"]*ipc\/(client|events)['"]/,
    why: '组件与视图只读 store（loading/错误态集中一处，不各写一遍）',
  },
];

function walk(dir) {
  const out = [];
  let names;
  try {
    names = readdirSync(dir);
  } catch {
    return out;
  }
  for (const name of names) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.(ts|svelte)$/.test(name) && !p.endsWith('.d.ts')) out.push(p);
  }
  return out;
}

const files = RULES.flatMap((rule) => rule.dirs.map((d) => ({ rule, dir: d })));
let violations = 0;
for (const { rule, dir } of files) {
  for (const file of walk(join(SRC, dir))) {
    const lines = readFileSync(file, 'utf8').split('\n');
    for (const [i, line] of lines.entries()) {
      if (!rule.re.test(line)) continue;
      violations++;
      console.error(
        `${relative(ROOT, file).replace(/\\/g, '/')}:${i + 1}  [${rule.id}] ${rule.why}\n    ${line.trim()}`
      );
    }
  }
}

if (violations) {
  console.error(`\nUI 分层门禁未通过：${violations} 处违规（规则见 tools/check-ui-layers.mjs）`);
  process.exit(1);
}
console.log('UI 分层门禁通过：components/views/stores 未绕过 lib/');

/* ---------------- z-index token 梯 ---------------- */

// 裸 z-index 一律违规；var(--z-*) 合法。tokens.css 是定义处（豁免）。
// CSS 与 svelte <style> 都扫；注释行（/* */ 续行）跳过。
// 判定用两段包含而不是单正则：`z-index:\s*(?!var...)` 的 \s* 会回退一格，
// 让负向前瞻落在空格上、把合法的 var 声明也判成违规（实测 18 处全误报）。
const zFiles = [];
(function zWalk(d) {
  for (const name of readdirSync(d)) {
    const p = join(d, name);
    if (statSync(p).isDirectory()) zWalk(p);
    else if (/\.(css|svelte)$/.test(name) && !p.endsWith('tokens.css')) zFiles.push(p);
  }
})(SRC);

let zViolations = 0;
for (const file of zFiles) {
  const lines = readFileSync(file, 'utf8').split('\n');
  for (const [i, line] of lines.entries()) {
    const t = line.trim();
    if (t.startsWith('*') || t.startsWith('/*') || t.startsWith('//')) continue;
    if (!/z-index:/.test(line) || /z-index:\s*var\(--z-/.test(line)) continue;
    zViolations++;
    console.error(
      `${relative(ROOT, file).replace(/\\/g, '/')}:${i + 1}  [z-ladder] z-index 必须取自 tokens.css 的 --z-* 阶梯\n    ${t}`
    );
  }
}

if (zViolations) {
  console.error(`\nz-index 阶梯门禁未通过：${zViolations} 处裸值`);
  process.exit(1);
}
console.log('z-index 阶梯门禁通过：全部取自 --z-* token');
