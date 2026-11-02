#!/usr/bin/env node
// 注释文风门禁。三类违例：高精度禁词、注释内日期戳、注释内 docs 引用——
// 引用单向化：历史与取舍查 ledger（docs 下的流水账目录），代码内不设路标。
// 宽泛词（像、别、坑、必须）误报率高，不进自动门禁，由治理批次人工处理。
//
// 范围：Rust（crates/、src-tauri/）、前端（ui/src/、ui/e2e/，排除 generated/）、
// 构建脚本（tools/*.mjs）。beets-tool 的 Python 侧由 tools/comment-gov 覆盖。
//
// 白名单：tools/check-style-allowlist.json（JSON 字符串数组）。
//   条目格式："相对路径"（整文件豁免）或 "相对路径:行号"（单行豁免），
//   路径使用 / 分隔。仅用于确属合理的保留项（如引用含禁词的文档标题）。
//
// 用法：node tools/check-style.mjs [--strict]
//   默认 report-only：输出命中清单与计数，退出码恒为 0（G7 起接入 verify）。
//   --strict：存在命中时退出码 1。
//
// 生成物目录（bindings/、ui/src/lib/ipc/generated/）的注释由 Rust 侧决定，
// 不在本门禁重复计数。

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const ROOT = join(import.meta.dirname, '..');
const SCAN_DIRS = ['crates', 'src-tauri', 'ui/src', 'ui/e2e', 'tools'];
const SKIP_DIRS = new Set([
  'node_modules', 'dist', 'target', 'test-results',
  'generated', 'binaries', 'gen', 'bindings',
]);
const EXTS = new Set(['.rs', '.ts', '.svelte', '.mjs']);

// 高精度禁词：口语词与拟人叙事（语域共同禁令）。
const BANNED = /(人话|拍板|顺手|翻车|搞定|抽风|收尸)/g;
// 注释内日期戳：交付物语域禁止历史叙事，日期是主要信号。
const DATE = /(20\d{2}-\d{2}-\d{2})/g;
// 注释内 docs 引用：引用单向化，代码内不设指向 docs 的路标。
// docs/NN 已覆盖其 § 小节形式；裸 § 是 GPL 条款/源码定位等合法引用，不拦。
const DOCREF = /(docs\/\d|notes\/|ADR-\d|AGENTS\.md)/g;

const strict = process.argv.includes('--strict');

function listFiles(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) listFiles(p, out);
    else if (EXTS.has(name.slice(name.lastIndexOf('.')))) out.push(p);
  }
  return out;
}

// 单行注释判定：返回 { isComment, isL2 }。isL2 = 契约层（rust ///、//! 与
// 块注释内部，含 JSDoc）——其内的引用/日期归 L2 收口批（延后桶），不阻断。
function makeCommentState(ext) {
  let inBlock = false;   // /* */
  let inHtml = false;    // <!-- -->
  return (line) => {
    const t = line.trimStart();
    const enteredBlock = inBlock;
    if (inHtml) {
      if (t.includes('-->')) inHtml = false;
      return { isComment: true, isL2: false };
    }
    if (inBlock) {
      if (t.includes('*/')) inBlock = false;
      return { isComment: true, isL2: true };
    }
    if (t.startsWith('/*')) {
      // 单行块注释 /* ... */ 与多行开块都算注释；开块且未闭合则进入续行状态。
      if (!t.includes('*/')) inBlock = true;
      return { isComment: true, isL2: true };
    }
    if (t.startsWith('<!--')) {
      if (!t.includes('-->')) inHtml = true;
      return { isComment: true, isL2: false };
    }
    if (t.startsWith('//')) {
      // 覆盖 rs 的 /// 与 //!、ts 的 //；契约层单独标记
      const isL2 = ext === '.rs' && (t.startsWith('///') || t.startsWith('//!'));
      return { isComment: true, isL2 };
    }
    if (ext === '.svelte' || ext === '.ts' || ext === '.mjs') {
      if (t.startsWith('*')) return { isComment: true, isL2: true }; // 块注释续行兜底
    }
    return { isComment: false, isL2: false };
  };
}

const files = SCAN_DIRS.flatMap((d) => listFiles(join(ROOT, d)));

let allowlist = [];
const allowPath = join(ROOT, 'tools', 'check-style-allowlist.json');
if (existsSync(allowPath)) {
  allowlist = JSON.parse(readFileSync(allowPath, 'utf8'));
}
const allowed = (relPath, lineNo) =>
  allowlist.includes(relPath) || allowlist.includes(`${relPath}:${lineNo}`);

const findings = [];
for (const file of files) {
  const relPath = relative(ROOT, file).split(sep).join('/');
  const ext = relPath.slice(relPath.lastIndexOf('.'));
  const isComment = makeCommentState(ext);
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);
  lines.forEach((line, i) => {
    const c = isComment(line);
    if (!c.isComment) return;
    const rel = `${relPath}:${i + 1}`;
    for (const re of [BANNED, DATE, DOCREF]) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(line)) !== null) {
        if (allowed(relPath, i + 1)) break;
        // 分桶：非契约层的日期/引用 = 阻断；契约层 = L2 延后；
        // 禁词 = 语域存量，report-only。
        const kind = re === BANNED ? 'banned' : re === DATE ? 'date' : 'docref';
        const bucket = kind === 'banned' ? 'report' : c.isL2 ? 'l2' : 'blocking';
        findings.push({ rel, tag: m[1], kind, bucket, text: line.trim().slice(0, 90) });
      }
    }
  });
}

for (const f of findings) {
  console.log(`${f.rel}  [${f.bucket}/${f.tag}]  ${f.text}`);
}
const blocking = findings.filter((f) => f.bucket === 'blocking').length;
const l2 = findings.filter((f) => f.bucket === 'l2').length;
const report = findings.filter((f) => f.bucket === 'report').length;
console.log(`\ncheck-style: 命中 ${findings.length}` +
  `（阻断 ${blocking} / L2 延后 ${l2} / 语域 report ${report}），扫描 ${files.length} 个文件` +
  (strict ? '（strict）' : '（report-only，不影响退出码）'));
// 阻断桶：非契约层注释里的日期戳与 docs 引用；--strict 时退出码 1
process.exit(strict && blocking > 0 ? 1 : 0);
