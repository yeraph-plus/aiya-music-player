#!/usr/bin/env node
// 边界门禁（docs/33 §3.2）：`crates/aiya-beets-client` 只许有"协议"，
// 不许有"环境知识"。
//
// 为什么需要机器检查：这条边界靠人自觉维持不住。想加一句"顺手在这里探测一下
// venv"太自然了，而一旦破口，分包就退化成"物理分开、逻辑还黏着"。
// 门禁的价值不在抓现行，在于让"想加"的那一刻先卡一下。
//
// 规则的取舍（比 docs/33 的字面清单更精确，原因写在每条后面）：
//   · `python` 例外 `pythonVersion` —— 那是协议线上字段名，改名就不是这个
//     协议了。crate 用 `runtime_version` 作为 Rust 标识符，只在 serde
//     rename 处出现真名，这正是"知道字段但不知道它意味着什么"的分界。
//   · `beets` 不作禁用词 —— 包名 `aiya-beets-client` 与协议字段
//     (`beetsVersion` / `beetsMessage`) 必然带它。禁的是**内部概念**
//     （插件名、库文件、对象模型），不是产品名。
//   · 用大小写敏感 + 词边界 —— 否则 `Stdio::piped` 会命中小写 pip，
//     `library_path` 会命中 `Library`，门禁变成噪音后就会被关掉。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), '..');
const TARGET = join(ROOT, 'crates/aiya-beets-client/src');

/** @type {{id:string, re:RegExp, why:string}[]} */
const RULES = [
  // —— 解释器 / 环境布局：crate 不该知道对面是用什么跑的 ——
  { id: 'venv', re: /\bvenv\b/, why: '虚拟环境是环境知识，属于工具侧' },
  { id: 'virtualenv', re: /\bvirtualenv\b/i, why: '同上' },
  { id: 'pyvenv.cfg', re: /pyvenv\.cfg/, why: 'venv 布局推断（父目录/位置）不在 client 里' },
  { id: 'site-packages', re: /site-packages/, why: '包目录布局属于工具侧' },
  { id: 'pip', re: /\bpip3?\b/, why: '包管理器属于工具侧（注意：Stdio::piped 不匹配）' },
  { id: 'python/py.exe', re: /python\.exe|\bpython3?\b(?!Version)/, why: '解释器形态属于工具侧（pythonVersion 是协议字段，豁免）' },
  { id: 'Scripts', re: /Scripts[\\/]/, why: '平台目录布局属于工具侧' },
  // —— 具体插件名：客户端不知道对面开了什么 ——
  {
    id: 'plugin-names',
    re: /\b(replaygain|lyrics|fromfilename|duplicates|scrub|smartplaylist|chroma|lastgenre|fetchart|embedart)\b/,
    why: '插件清单由工具侧自报（capabilities），client 不硬编码',
  },
  // —— 工具自己的持久化细节 ——
  { id: 'library.db', re: /library\.db/, why: '库文件名属于工具侧' },
  { id: 'config.yaml', re: /config\.yaml/, why: '配置文件格式属于工具侧' },
  { id: 'path_formats', re: /path_formats/, why: '整理规则属于工具侧' },
  // —— 工具的对象模型 ——
  { id: 'domain-objects', re: /\b(Library|MoveOperation|ImportSession)\b/, why: '对象模型属于工具侧，client 只认 JSON' },
];

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (name.endsWith('.rs')) out.push(p);
  }
  return out;
}

let violations = 0;
for (const file of walk(TARGET)) {
  const lines = readFileSync(file, 'utf8').split('\n');
  for (const [i, line] of lines.entries()) {
    for (const rule of RULES) {
      if (!rule.re.test(line)) continue;
      // 豁免：serde rename 到协议真名字段。这是唯一允许"提到"的地方，
      // 因为它描述的是**线上字段**，不是环境。
      if (rule.id === 'python/py.exe' && /rename\s*=\s*"pythonVersion"/.test(line)) continue;
      violations++;
      console.error(
        `${relative(ROOT, file)}:${i + 1}  [${rule.id}] ${rule.why}\n    ${line.trim()}`
      );
    }
  }
}

if (violations) {
  console.error(`\n边界门禁未通过：${violations} 处违规（规则见 docs/33 §3.2）`);
  process.exit(1);
}

/* ------------------------------------------------------------------ *
 * 第二道：核心链路不许依赖 beets 工具（docs/33 §3.1 硬原则）
 *
 * 「beets 是增强能力，不是播放依赖」这句话如果只在文档里，第一天就会被
 * 一句 `use aiya_beets_client::…` 破坏——而且破坏之后**装了的机器上一切正常**，
 * 只有没装的用户会挂。所以把它变成门禁：音频引擎、播放器、数据层连
 * 依赖都不许有，于是"未装 beets 时核心全通"是编译期事实，不是承诺。
 *
 * 只查直接依赖就够：这四个 crate 之间不互相引 beets client，且它们的
 * 依赖闭包全在 crates/ 内部（外部 crate 不会 path 依赖我们的私有 client）。
 * ------------------------------------------------------------------ */
const CORE_CRATES = ['aiya-audio', 'aiya-audio-host', 'aiya-player', 'aiya-data'];
const beetsDep = /aiya-beets-client|aiya_beets_client/;

let leaks = 0;
for (const name of CORE_CRATES) {
  const manifest = join(ROOT, 'crates', name, 'Cargo.toml');
  let text;
  try {
    text = readFileSync(manifest, 'utf8');
  } catch {
    continue; // crate 不存在（裁剪构建）就跳过
  }
  const lines = text.split('\n');
  for (const [i, line] of lines.entries()) {
    // 注释行不算依赖
    if (/^\s*#/.test(line)) continue;
    if (!beetsDep.test(line)) continue;
    leaks++;
    console.error(
      `crates/${name}/Cargo.toml:${i + 1}  核心链路不许依赖 beets 工具` +
        `（未装 beets 的用户也要能完整播放）\n    ${line.trim()}`
    );
  }
}

// 旧实现必须彻底消失：目录还在就说明有人又照着旧路径写代码了
const staleDirs = ['sidecar', 'tools/beets-runtime'];
for (const rel of staleDirs) {
  try {
    statSync(join(ROOT, rel));
    leaks++;
    console.error(`${rel}/ 仍然存在：beets 侧已迁到 beets-tool/（docs/33 §9.0）`);
  } catch {
    /* 不存在 = 正确 */
  }
}

if (leaks) {
  console.error(`\n边界门禁未通过：${leaks} 处依赖越界（规则见 docs/33 §3.1）`);
  process.exit(1);
}

console.log(
  `边界门禁通过：crates/aiya-beets-client/src 无环境知识泄漏（${RULES.length} 条规则）；` +
    `核心链路（${CORE_CRATES.join('/')}）不依赖 beets 工具`
);
