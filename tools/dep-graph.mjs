#!/usr/bin/env node
/**
 * 模块依赖图 · 环检测。
 *
 * 为什么需要它：store 之间有一条隐性契约 —— **依赖只许自上而下**。
 * `library`（只依赖 ipc）← `list`/`scan` ← `playlists` ← 视图。
 * 2026-10-26 把歌单摘要从 `ui` 挪到 `library`，就是因为想挪进 `playlists`
 * 会让 `list/scan → playlists → list` 成环。当时用手写脚本验的，这里有
 * 固化版：改完 store 跑一次，条件反射。
 *
 * 环在 ESM 里不一定会立刻炸（两个单例只在方法体里互相访问时能跑通），
 * 所以它比崩溃更麻烦：**跑得起来，但初始化顺序变得不可推理**。
 *
 * 用法：`npm run check:deps`（在 ui/ 下）或 `node ../../tools/dep-graph.mjs`
 * 退出码：有环 = 1，没有 = 0；同时把最长的几条依赖链打出来。
 */
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, normalize, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src');

/** @param {string} dir @returns {string[]} */
function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      // generated/ 是 ts-rs 产物，mock/ 是预览后端，两者都不参与架构约束
      if (name === 'generated' || name === 'mock') continue;
      out.push(...walk(path));
    } else if (name.endsWith('.ts') || name.endsWith('.svelte')) {
      out.push(path);
    }
  }
  return out;
}

const files = walk(root);
const fileSet = new Set(files);
const IMPORT = /from\s+['"]([^'"]+)['"]/g;

/** @param {string} from @param {string} spec */
function resolveSpec(from, spec) {
  if (!spec.startsWith('.')) return null; // 只关心内部依赖
  const base = normalize(join(dirname(from), spec));
  for (const candidate of [base, `${base}.ts`, `${base}.svelte`, join(base, 'index.ts')]) {
    if (fileSet.has(candidate)) return candidate;
  }
  return null;
}

/** @type {Map<string, string[]>} */
const graph = new Map();
for (const file of files) {
  const text = readFileSync(file, 'utf8');
  const deps = [];
  for (const match of text.matchAll(IMPORT)) {
    const target = resolveSpec(file, match[1]);
    if (target) deps.push(target);
  }
  graph.set(file, deps);
}

/** @type {string[][]} */
const cycles = [];
/** @param {string} node @param {string[]} stack @param {Set<string>} done */
function dfs(node, stack, done) {
  if (stack.includes(node)) {
    cycles.push([...stack.slice(stack.indexOf(node)), node]);
    return;
  }
  if (done.has(node)) return;
  const at = stack.length;
  stack.push(node);
  for (const next of graph.get(node) ?? []) dfs(next, stack, done);
  stack.length = at;
  done.add(node);
}

const done = new Set();
for (const file of files) dfs(file, [], done);

const short = (p) => relative(root, p).replace(/\\/g, '/');
const edges = [...graph.values()].reduce((sum, deps) => sum + deps.length, 0);

if (cycles.length > 0) {
  // 同一个环会因为起点不同被找到多次，按成员集合去重
  const unique = new Map();
  for (const cycle of cycles) {
    unique.set([...new Set(cycle)].sort().join('|'), cycle);
  }
  console.error(`✗ 发现 ${unique.size} 个循环依赖：`);
  for (const cycle of unique.values()) {
    console.error(`  ${cycle.map(short).join(' → ')}`);
  }
  process.exit(1);
}

console.log(`✓ 无循环依赖（${files.length} 个模块 / ${edges} 条内部依赖）`);
