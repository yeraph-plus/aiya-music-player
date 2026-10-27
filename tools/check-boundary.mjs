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
console.log(`边界门禁通过：crates/aiya-beets-client/src 无环境知识泄漏（${RULES.length} 条规则）`);
