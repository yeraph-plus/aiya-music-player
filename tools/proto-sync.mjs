#!/usr/bin/env node
/**
 * 协议镜像同步 / 校验（docs/33 §4.4）。
 *
 * 真源在工具侧（`beets-tool/protocol/v2.schema.json`），播放器仓根镜像一份。
 * 镜像**必须是字节副本**，不是"我以为等价"的重排版本——重排会让 diff 看着
 * 一样、实际 $defs 顺序变了。
 *
 *   npm run proto:sync    把真源复制过来，重算 sha256 与版本元数据
 *   npm run proto:check   只校验（CI 用），不一致就 exit 1
 *
 * 为什么同步是**手动动作**：协议变化必须在 diff 里看得见。任何"自动跟随"
 * 都会让契约漂移变成静默事件。
 *
 * 这个脚本**不调用 Python**：`proto:check` 要能在没有 Python 的机器上跑
 * （播放器侧的常规门禁不该依赖工具链）。
 */
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SRC = resolve(ROOT, 'beets-tool/protocol/v2.schema.json');
const MIRROR = resolve(ROOT, 'protocol/aiya-beets-v2.json');
const META = resolve(ROOT, 'protocol/aiya-beets-v2.meta.json');
const PROTOCOL_PY = resolve(ROOT, 'beets-tool/src/aiya_beets/protocol.py');
const INIT_PY = resolve(ROOT, 'beets-tool/src/aiya_beets/__init__.py');

const isCheck = process.argv.includes('--check');
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

/** 从 Python 源码里抠一个常量。不 import：门禁不该需要解释器。 */
function pyConst(file, name) {
  const text = readFileSync(file, 'utf8');
  const m = text.match(new RegExp(`^${name}\\s*=\\s*"?([^"\\n]+)"?`, 'm'));
  if (!m) throw new Error(`在 ${file} 里找不到 ${name}`);
  return m[1].trim();
}

const srcBytes = readFileSync(SRC);
const srcHash = sha256(srcBytes);
const protocolVersion = Number(pyConst(PROTOCOL_PY, 'PROTOCOL_VERSION'));
const toolVersion = pyConst(INIT_PY, '__version__');

if (!Number.isInteger(protocolVersion) || protocolVersion < 1) {
  throw new Error(`PROTOCOL_VERSION 不是正整数：${protocolVersion}`);
}

if (!isCheck) {
  mkdirSync(dirname(MIRROR), { recursive: true });
  writeFileSync(MIRROR, srcBytes);
  const meta = {
    source: 'beets-tool/protocol/v2.schema.json',
    protocolVersion,
    toolVersion,
    tag: `beets-portable-v${toolVersion}`,
    sha256: srcHash,
    note: '字节副本；改协议请改真源后跑 npm run proto:sync，并在 diff 里看变化',
  };
  writeFileSync(META, JSON.stringify(meta, null, 2) + '\n');
  console.log(`镜像已更新：protocol/aiya-beets-v2.json  sha256=${srcHash.slice(0, 12)}…  v${protocolVersion}`);
  process.exit(0);
}

/* ---------------- --check ---------------- */
const problems = [];

let mirrorBytes;
try {
  mirrorBytes = readFileSync(MIRROR);
} catch {
  problems.push(`镜像不存在：${MIRROR}（跑 npm run proto:sync）`);
}

let meta;
try {
  meta = JSON.parse(readFileSync(META, 'utf8'));
} catch {
  problems.push(`元数据不存在或不是合法 JSON：${META}`);
}

if (mirrorBytes) {
  const mirrorHash = sha256(mirrorBytes);
  if (mirrorHash !== srcHash) {
    problems.push(
      `镜像与真源字节不一致：\n    真源 ${srcHash}\n    镜像 ${mirrorHash}\n` +
      '    → 跑 npm run proto:sync（协议变了就必须在 diff 里看得见）',
    );
  }
  if (meta && meta.sha256 !== mirrorHash) {
    problems.push(`元数据 sha256 与镜像不符：meta=${meta.sha256} 实际=${mirrorHash}`);
  }
  // 镜像必须是可解析的 JSON：副本坏掉时上面那条会过（两边一样坏），这条兜住
  try {
    const doc = JSON.parse(mirrorBytes.toString('utf8'));
    for (const name of ['Request', 'Response', 'Event', 'InstallerEvent', 'StatusData', 'ErrorCode']) {
      if (!doc.$defs?.[name]) problems.push(`镜像缺 $defs.${name}`);
    }
    if (doc.$defs?.StatusData?.properties?.schemaHash === undefined) {
      problems.push('镜像缺 StatusData.schemaHash（客户端靠它做运行时比对）');
    }
  } catch (e) {
    problems.push(`镜像不是合法 JSON：${e.message}`);
  }
}

if (meta) {
  if (meta.protocolVersion !== protocolVersion) {
    problems.push(
      `协议版本不一致：镜像 meta=${meta.protocolVersion} 工具源码=${protocolVersion}` +
      '（版本变了要一起改：proto.py、schema、镜像）',
    );
  }
  if (meta.toolVersion !== toolVersion) {
    problems.push(`工具版本不一致：meta=${meta.toolVersion} 源码=${toolVersion}（跑 npm run proto:sync）`);
  }
}

if (problems.length) {
  console.error('协议镜像校验未通过：');
  for (const p of problems) console.error(`  · ${p}`);
  process.exit(1);
}
console.log(`协议镜像一致：v${protocolVersion}  sha256=${srcHash.slice(0, 12)}…  （${SRC.split(/[\\/]/).slice(-2).join('/')}）`);
