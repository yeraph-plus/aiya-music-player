#!/usr/bin/env node
// 从 Rust 生成 TypeScript 类型（docs/17 §6：Rust 是唯一源）。
//
// 为什么用 `cargo test` 当生成器：ts-rs 的 `#[ts(export)]` 会为每个类型生成一个
// `export_bindings_<type>` 测试，跑测试就是把文件写出来。好处是不用另建二进制、
// 也不用手写序列化器；代价是必须先编译一遍测试（debug 增量，几秒）。
//
// 生成物**入库**：`npm run gen:types:check` 先生成到临时目录再逐文件比对现有产物，
// "改了 Rust 忘了更新类型"会直接红，而不是等到运行期字段对不上。
//
// 注意：仓库目前**没有 CI**（`.github/` 不存在），这条门禁只在本地 `npm run verify`
// 里跑。接入 CI 后应把 verify（含本检查）设为 PR 必过项 —— 见 docs/30 §3 P1-6。
//
// 用法：node tools/gen-types.mjs [--check]
//   --check  生成到临时目录并与现有文件比较，不写工作区（CI 用）

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'ui', 'src', 'lib', 'ipc', 'generated');
const check = process.argv.includes('--check');

const env = { ...process.env };
const cargoBin = join(process.env.USERPROFILE || process.env.HOME || '', '.cargo', 'bin');
if (!(env.PATH || '').toLowerCase().includes(cargoBin.toLowerCase())) {
  env.PATH = `${cargoBin}${process.platform === 'win32' ? ';' : ':'}${env.PATH || ''}`;
}

const target = check ? mkdtempSync(join(tmpdir(), 'aiya-types-')) : outDir;
mkdirSync(target, { recursive: true });

// 三个 crate 都有导出：aiya（命令 DTO）、aiya-player（播放状态）、aiya-data（曲目）
const crates = ['aiya', 'aiya-player', 'aiya-data'];
const args = ['test', '--quiet'];
for (const c of crates) args.push('-p', c);
args.push('export_bindings');

try {
  execFileSync('cargo', args, {
    cwd: root,
    stdio: check ? 'pipe' : 'inherit',
    env: { ...env, TS_RS_EXPORT_DIR: target },
  });
} catch (e) {
  console.error('[gen-types] 生成失败：');
  if (e.stdout) console.error(e.stdout.toString().slice(-4000));
  if (e.stderr) console.error(e.stderr.toString().slice(-4000));
  process.exit(1);
}

const files = readdirSync(target).filter((f) => f.endsWith('.ts') && f !== 'index.ts').sort();
if (files.length === 0) {
  console.error('[gen-types] 没有生成任何文件——检查 #[ts(export)] 是否还在');
  process.exit(1);
}

// 桶文件也由本脚本生成：类型有 41 个之后，手写 `export * from './XXX'` 的清单
// 一定会漏（漏了不报错，只是 `import { PlaylistSummary } from '$lib/ipc'` 找不到）。
const BANNER = '// 由 npm run gen:types 生成，勿手改。\n';
const indexSrc =
  BANNER + files.map((f) => `export * from './${f.replace(/\.ts$/, '')}';`).join('\n') + '\n';
writeFileSync(join(target, 'index.ts'), indexSrc);
files.push('index.ts');

if (!check) {
  console.log(`[gen-types] ${files.length - 1} 个类型 + index.ts → ${outDir}`);
  process.exit(0);
}

// --check：逐个文件比对，不一致就列出并失败
const diffs = [];
for (const f of files) {
  const a = join(target, f);
  const b = join(outDir, f);
  if (!existsSync(b)) diffs.push(`新增 ${f}`);
  else if (readFileSync(a, 'utf8') !== readFileSync(b, 'utf8')) diffs.push(`变更 ${f}`);
}
const stale = existsSync(outDir)
  ? readdirSync(outDir).filter((f) => f.endsWith('.ts') && !files.includes(f))
  : [];
for (const f of stale) diffs.push(`多余 ${f}`);
rmSync(target, { recursive: true, force: true });

if (diffs.length) {
  console.error('[gen-types] 类型与 Rust 不一致，请运行 npm run gen:types：');
  for (const d of diffs) console.error(`  - ${d}`);
  process.exit(1);
}
console.log(`[gen-types] ✓ ${files.length - 1} 个类型与 Rust 一致`);
