// tools/run.mjs — npm scripts 的统一执行入口：补齐工具链目录后转发一条命令。
//
// 为什么需要这一层：`npm run` 在 Windows 上是 cmd.exe 执行脚本，拿到的 PATH
// 取决于**谁启动了 npm**。从 VS Code / PowerShell / CMD 里跑时经常没有
// `%USERPROFILE%\.cargo\bin`（rustup 默认只写用户 PATH 的某一层），于是
// `npm run test` 报「'cargo' 不是内部或外部命令」。脚本不该依赖调用者的
// shell 配置，所以这里把已知安装位置前置进 PATH 再 spawn。
//
// 用法：node tools/run.mjs <command> [args...]
//   node tools/run.mjs cargo test --workspace
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { delimiter, join } from 'node:path';

const home = process.env.USERPROFILE ?? process.env.HOME ?? '';

// 按优先级排列；只保留真实存在的目录（避免往 PATH 里塞垃圾路径）
const candidates = [
  process.env.CARGO_HOME ? join(process.env.CARGO_HOME, 'bin') : '',
  home ? join(home, '.cargo', 'bin') : '',
  'C:\\Program Files\\nodejs',
];
const prepend = [...new Set(candidates.filter(Boolean))].filter((p) => existsSync(p));

// Windows 的 PATH 可能是 Path / PATH 任意大小写；先剔除所有写法再加一份，
// 否则会出现「两份 PATH 并存、后者覆盖前者」的诡异行为。
const env = {};
for (const [k, v] of Object.entries(process.env)) {
  if (k.toUpperCase() !== 'PATH') env[k] = v;
}
env.PATH = [...prepend, process.env.PATH ?? ''].filter(Boolean).join(delimiter);

const [cmd, ...args] = process.argv.slice(2);
if (!cmd) {
  console.error('用法: node tools/run.mjs <command> [args...]');
  process.exit(2);
}

// 直接给 .js/.mjs 时用当前 node 解释器跑（省掉再拼一次 `node`）；
// .cmd/.bat 无法被 CreateProcess 直接执行（Node 不带 shell 时会 ENOENT）。
const isScript = /\.(c|m)?js$/i.test(cmd);
const useShell = /\.(cmd|bat)$/i.test(cmd);
const [file, argv] = isScript ? [process.execPath, [cmd, ...args]] : [cmd, args];
const res = spawnSync(file, argv, { stdio: 'inherit', env, shell: useShell });

if (res.error) {
  console.error(`\n[run] 无法执行 ${cmd}: ${res.error.message}`);
  console.error(`[run] 已前置到 PATH: ${prepend.join(delimiter) || '(无)'}`);
  console.error('[run] Rust 工具链缺失时先装：https://rustup.rs');
  process.exit(127);
}
if (res.signal) {
  console.error(`\n[run] ${cmd} 被信号终止: ${res.signal}`);
  process.exit(1);
}
process.exit(res.status ?? 0);
