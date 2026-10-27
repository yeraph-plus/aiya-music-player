#!/usr/bin/env node
// 打便携版：把 release 产物收成一个「解压即用」的 zip。
//
// ## 为什么要它
//
// 安装包（NSIS）在 Windows 上必然触发 SmartScreen「未知发布者」——
// 没有代码签名证书时这个提示无法避免（本项目的决定：不买证书，如实提示，
// 同时给一个不需要安装、不需要写注册表的版本）。便携版还有第二个用处：
// 用户可以把它丢进 U 盘，数据全在 data/ 下，拔盘即走。
//
// ## 里有什么、为什么
//
//   AIYA Music.exe      ← 主程序（与安装版同名，任务管理器里对得上）
//   aiya-audio-host.exe ← 引擎进程，`sidecar.rs` 的 locate_host_exe 找**同目录**
//                         的这个名字，所以它不能进子目录
//   portable.txt        ← 便携标记，`paths::portable_data_root` 只认这个文件
//   README.txt          ← 「未知发布者」怎么处理、数据在哪、许可
//   licenses/           ← 与 NSIS 的 resources 一一对应（GPL 要求随包附全文）
//
// ## 为什么自己写 zip（而不是调 PowerShell Compress-Archive）
//
// 1) `Compress-Archive` 会丢掉空目录、把路径分隔符写成 `\`（部分解压器不认），
//    而且对大文件是整体读进内存；
// 2) 项目里所有 tools/*.mjs 都只用 Node 内置模块，不引依赖；
// 3) zip 容器本身很小：deflate 是 Node 内置的，缺的只有 CRC32 和头部拼装。
//
// 用法：node tools/build-portable.mjs [--no-zip]
// 前置：`npm run bundle`（要 target/release 下的两个 exe）

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateRawSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const product = conf.productName;                 // "AIYA Music"
const version = conf.version;
const bundleDir = join(root, "target", "release", "bundle", "portable");
const stage = join(bundleDir, `${product}-${version}-portable`);
const out = join(bundleDir, `${product.replace(/ /g, "-")}_${version}_x64-portable.zip`);

function fail(msg) {
  console.error(`[portable] ${msg}`);
  process.exit(1);
}

// ---------------------------------------------------------------- 收集文件
const mainExe = join(root, "target", "release", "aiya.exe");
const hostExe = join(root, "target", "release", "aiya-audio-host.exe");
for (const [p, hint] of [
  [mainExe, "先跑 `npm run bundle`"],
  [hostExe, "先跑 `npm run build:sidecar`"],
]) {
  if (!existsSync(p)) fail(`缺少 ${p}（${hint}）`);
}

// 与 tauri.conf.json 的 bundle.resources 保持一致：包里许可不全会踩 GPL §4/§6
const licenseSrc = join(root, "licenses");
const licenseFiles = [
  [join(root, "LICENSE"), join("licenses", "LICENSE")],
  ...readdirSync(licenseSrc)
    .filter((f) => f.endsWith(".md") || f.endsWith(".txt"))
    .map((f) => [join(licenseSrc, f), join("licenses", f)]),
];

const readme = `AIYA Music ${version} — 便携版（免安装）
${"=".repeat(40)}

解压到任意目录，双击 "${product}.exe" 即可运行。不写注册表，不需要管理员权限。

数据在哪
--------
全部在本目录下的 data\\（首次运行时自动创建）：

  library.sqlite    曲库数据库
  cache\\artwork\\    封面缓存
  logs\\             运行日志（出问题时最先看这里）
  beets\\            整理库：beets 的库与配置
  beets-tool\\       安装的 beets 工具本体
  webview\\          界面引擎（WebView2）的缓存

搬运＝复制整个文件夹；卸载＝删掉整个文件夹。没有任何东西留在系统盘。
想改回「数据存在系统用户目录」：删掉本目录的 portable.txt
（已有数据不会自动搬过去，需要手动移动 data\\ 里的内容）。

Windows「未知发布者 / 已保护你的电脑」
--------------------------------------
本程序没有代码签名证书（开源个人项目，证书要按年付费），首次运行时
SmartScreen 会拦一下，这是预期行为，不是文件损坏：

  点「更多信息」→「仍要运行」

核对来源：https://github.com/yeraph-plus/aiya-music-player

首次启动会创建曲库；到「设置 → 曲库」添加你的音乐目录后开始扫描。

许可
----
GPL-3.0-or-later，全文见 licenses\\LICENSE
第三方组件：licenses\\THIRD-PARTY-NOTICES.md（Rust）/ licenses\\CARGO-DEPS.md
源码获取方式：licenses\\SOURCE-OFFER.txt
`;

// ---------------------------------------------------------------- 展开目录
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });

function put(src, rel) {
  const dest = join(stage, rel);
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(src, dest);
}

put(mainExe, `${product}.exe`);
put(hostExe, "aiya-audio-host.exe");
for (const [src, rel] of licenseFiles) put(src, rel);
writeFileSync(join(stage, "portable.txt"), `${product} ${version} 便携版\n`, "utf8");
writeFileSync(join(stage, "README.txt"), readme.replace(/\n/g, "\r\n"), "utf8");

// 调试用：只展开目录，不打 zip（以前这个判断放在最后，参数名与实际行为不符）
if (process.argv.includes("--no-zip")) {
  console.log(`[portable] OK（未打包） ${relative(root, stage)}`);
  process.exit(0);
}

// ---------------------------------------------------------------- zip 写入
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function dosTime(d) {
  const time = (d.getHours() << 11) | (d.getMinutes() << 5) | (d.getSeconds() >> 1);
  const date = ((d.getFullYear() - 1980) << 9) | ((d.getMonth() + 1) << 5) | d.getDate();
  return { time, date };
}

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

const now = new Date();
const { time: dTime, date: dDate } = dosTime(now);
const chunks = [];
const central = [];
let offset = 0;
let entries = 0; // 条目数（别用 central.length/2 反推：以后往 central 里多 push 一项就错）

for (const abs of walk(stage)) {
  // zip 内路径用 /（Windows 解压器也认），顶层带一个目录名，解压不散落
  const name = `${product}-${version}-portable/${relative(stage, abs).replace(/\\/g, "/")}`;
  const nameBuf = Buffer.from(name, "utf8");
  const data = readFileSync(abs);
  const crc = crc32(data);
  const packed = deflateRawSync(data, { level: 6 });
  const useDeflate = packed.length < data.length;
  const payload = useDeflate ? packed : data;

  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4); // 解压所需版本 2.0
  local.writeUInt16LE(0x0800, 6); // bit 11：文件名是 UTF-8
  local.writeUInt16LE(useDeflate ? 8 : 0, 8);
  local.writeUInt16LE(dTime, 10);
  local.writeUInt16LE(dDate, 12);
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(payload.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(nameBuf.length, 26);
  local.writeUInt16LE(0, 28);
  chunks.push(local, nameBuf, payload);

  const cd = Buffer.alloc(46);
  cd.writeUInt32LE(0x02014b50, 0);
  cd.writeUInt16LE(20, 4); // 制作版本
  cd.writeUInt16LE(20, 6); // 解压所需版本
  cd.writeUInt16LE(0x0800, 8);
  cd.writeUInt16LE(useDeflate ? 8 : 0, 10);
  cd.writeUInt16LE(dTime, 12);
  cd.writeUInt16LE(dDate, 14);
  cd.writeUInt32LE(crc, 16);
  cd.writeUInt32LE(payload.length, 20);
  cd.writeUInt32LE(data.length, 24);
  cd.writeUInt16LE(nameBuf.length, 28);
  cd.writeUInt32LE(0, 38); // 外部属性（文件）
  cd.writeUInt32LE(offset, 42);
  central.push(cd, nameBuf);
  entries += 1;

  offset += local.length + nameBuf.length + payload.length;
}

const cdBuf = Buffer.concat(central);
const eocd = Buffer.alloc(22);
eocd.writeUInt32LE(0x06054b50, 0);
eocd.writeUInt16LE(entries, 8);
eocd.writeUInt16LE(entries, 10);
eocd.writeUInt32LE(cdBuf.length, 12);
eocd.writeUInt32LE(offset, 16);

mkdirSync(bundleDir, { recursive: true });
writeFileSync(out, Buffer.concat([...chunks, cdBuf, eocd]));

// ---------------------------------------------------------------- 自检
// 把产物**读回来**再验一遍：拼 zip 头部最容易错的就是偏移量与中央目录，
// 写完不解一次等于没验。
//
// 为什么不用 tar / Expand-Archive：GNU tar 根本不认 zip（"This does not look
// like a tar archive"），而且 `D:\...` 会被 tar 当成「远程主机:路径」
// （Cannot connect to D）；换 PowerShell 又要多一层 shell 与路径转义。
// 读中央目录只有十几行，验的还是自己写进去的那几个字节。
function readZipNames(buf) {
  let eocdAt = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 65557); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocdAt = i;
      break;
    }
  }
  if (eocdAt < 0) throw new Error("找不到 EOCD（不是 zip）");
  const count = buf.readUInt16LE(eocdAt + 10);
  let p = buf.readUInt32LE(eocdAt + 16);
  const names = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(p) !== 0x02014b50) throw new Error(`中央目录第 ${i} 项签名不对 @${p}`);
    const nameLen = buf.readUInt16LE(p + 28);
    names.push(buf.subarray(p + 46, p + 46 + nameLen).toString("utf8"));
    p += 46 + nameLen + buf.readUInt16LE(p + 30) + buf.readUInt16LE(p + 32);
  }
  return names;
}

let listing;
try {
  listing = readZipNames(readFileSync(out));
} catch (e) {
  fail(`自检失败：${e.message}`);
}

const need = [
  `${product}.exe`,
  "aiya-audio-host.exe",
  "portable.txt",
  "README.txt",
  "licenses/LICENSE",
  "licenses/SOURCE-OFFER.txt",
];
const joined = listing.join("\n");
const missing = need.filter((n) => !joined.includes(n));
if (missing.length) fail(`自检失败：zip 里缺 ${missing.join(", ")}`);

const staged = walk(stage).length;
if (listing.length !== staged) {
  fail(`自检失败：条目数 ${listing.length} ≠ 待打包文件数 ${staged}`);
}
// 便携标记与 sidecar 必须与主程序**同级**，否则 sidecar.rs 找不到引擎
for (const n of [`${product}.exe`, "aiya-audio-host.exe", "portable.txt"]) {
  if (!listing.some((e) => e.endsWith(`/${n}`) && e.split("/").length === 2)) {
    fail(`自检失败：${n} 不在压缩包顶层目录里`);
  }
}

const mb = (statSync(out).size / (1024 * 1024)).toFixed(1);
console.log(`[portable] ${relative(root, out)}  ${mb} MB  ${listing.length} 个条目`);
