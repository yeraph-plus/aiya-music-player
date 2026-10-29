#!/usr/bin/env node
// 生成 licenses/CARGO-DEPS.md：Rust 依赖全量清单（名称/版本/许可/来源）。
//
// 为什么要生成而不是手写：直接依赖十几个、传递依赖数百个，手写必然过期。
// GPL-3.0 分发时"完整、对应版本的依赖清单"是合规底线，生成物随版本走。
//
// 用法：node tools/gen-notices.mjs   （需要 cargo 在 PATH，可用 --offline）

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

// cargo 可能不在默认 PATH（Windows 上装在 %USERPROFILE%\.cargo\bin）
const env = { ...process.env };
const cargoBin = join(process.env.USERPROFILE || process.env.HOME || "", ".cargo", "bin");
if (!(env.PATH || "").toLowerCase().includes(cargoBin.toLowerCase())) {
  env.PATH = `${cargoBin};${env.PATH || ""}`;
}

const meta = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--offline"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    env,
  }),
);

const own = new Set(
  meta.packages.filter((p) => p.source === null).map((p) => `${p.name}@${p.version}`),
);

const rows = meta.packages
  .filter((p) => p.source && p.source.includes("registry"))
  .map((p) => ({
    name: p.name,
    version: p.version,
    license: p.license || (p.license_file ? "(见 license-file)" : "UNKNOWN"),
    repo: (p.repository || "").replace(/^https?:\/\//, ""),
  }))
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const byLicense = new Map();
for (const r of rows) byLicense.set(r.license, (byLicense.get(r.license) || 0) + 1);

const md = [
  "# Rust 依赖清单（自动生成，勿手改）",
  "",
  `生成时间：${new Date().toISOString().slice(0, 10)}　条目数：${rows.length}`,
  "生成命令：`node tools/gen-notices.mjs`",
  "",
  "本仓库自有的 " + own.size + " 个 crate（" + [...own].sort().join("、") + "）以 GPL-3.0-or-later 发布，",
  "不在此表内。下表为 crates.io 上的第三方依赖，均已静态链接进 `aiya.exe`",
  "或 `aiya-audio-host.exe`。",
  "",
  "## 许可分布",
  "",
  "| 许可 | 条目数 |",
  "|---|---|",
  ...[...byLicense.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([l, n]) => `| ${l} | ${n} |`),
  "",
  "## 全量清单",
  "",
  "| crate | 版本 | 许可 | 仓库 |",
  "|---|---|---|---|",
  ...rows.map((r) => `| ${r.name} | ${r.version} | ${r.license} | ${r.repo} |`),
  "",
];

writeFileSync(join(root, "licenses", "CARGO-DEPS.md"), md.join("\n"), "utf8");
console.log(`licenses/CARGO-DEPS.md：${rows.length} 条，${byLicense.size} 种许可`);
