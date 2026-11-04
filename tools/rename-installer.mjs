#!/usr/bin/env node
// 安装包产物名去空格：tauri 按 productName 生成 "AIYA Music_<ver>_x64-setup.exe"，
// 分发文件名统一连字符（与便携 zip 的 AIYA-Music_... 同款）。只改文件名，
// 安装器内容不动；目录里没有待改文件（重复跑/产物缺失）就静默通过。
import { existsSync, readdirSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "target", "release", "bundle", "nsis");
if (!existsSync(dir)) process.exit(0);
for (const f of readdirSync(dir)) {
  if (f.endsWith("-setup.exe") && f.includes(" ")) {
    const to = f.replaceAll(" ", "-");
    renameSync(join(dir, f), join(dir, to));
    console.log(`[rename] ${f} -> ${to}`);
  }
}
