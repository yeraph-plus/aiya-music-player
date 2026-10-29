#!/usr/bin/env node
// 打包前置：编出 aiya-audio-host 并放到 Tauri externalBin 认得的路径。
//
// 为什么需要这一步：
// 1) sidecar 不是 aiya.exe 的依赖，`tauri build` 不会顺带编它；
// 2) Tauri 的 externalBin 要求文件名带**目标三元组后缀**
//    （`aiya-audio-host-x86_64-pc-windows-msvc.exe`），复制进安装目录时
//    才会被剥成 `aiya-audio-host.exe` —— 宿主 `locate_host_exe()` 找的正是
//    主程序同目录的这个名字；
// 3) profile 必须与主程序一致（release），否则安装包里混进一个 debug 引擎，
//    体积大 10 倍、性能也不对。
//
// 用法：node tools/build-sidecar.mjs [--release|--debug]

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const isWin = process.platform === "win32";
const profile = process.argv.includes("--debug") ? "debug" : "release";
const exeSuffix = isWin ? ".exe" : "";

const env = { ...process.env };
const cargoBin = join(process.env.USERPROFILE || process.env.HOME || "", ".cargo", "bin");
if (!(env.PATH || "").toLowerCase().includes(cargoBin.toLowerCase())) {
  env.PATH = `${cargoBin}${isWin ? ";" : ":"}${env.PATH || ""}`;
}

// panic 恢复策略（ADR-0002 守护链的一环）：sidecar 任何线程 panic 都必须
// 升级成进程死亡。宿主看门狗只认心跳（5s 超时 → kill → 换代重连 → 恢复现场），
// 认不出"引擎线程已 panic 但进程还活着"的僵尸——心跳照跳、声音永远静默。
// panic=abort 让任意线程的 panic 直接 abort：心跳停 → 看门狗接管换代重建
// （既有已测试的路径）。engine.rs 的 catch_unwind 兜底只服务于绕过本脚本的
// 裸跑场景（示例/调试），不是主防线。
// debug 与 release 两条路径共用下面的 env，一处即全覆盖。
// 代价：改变本 crate 全部依赖的编译指纹（首次构建全量重编），属预期。
// 追加而非覆盖 RUSTFLAGS，保留调用方（CI/用户）已有的 flags。
const panicAbort = "-C panic=abort";
env.RUSTFLAGS = env.RUSTFLAGS ? `${env.RUSTFLAGS} ${panicAbort}` : panicAbort;

// 目标三元组：拿 rustc 的 host（交叉编译时用 --target 覆盖会更复杂，
// v1 只出 Windows x64，先按 host 走）
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8", env })
  .split(/\r?\n/)
  .find((l) => l.startsWith("host: "))
  .slice("host: ".length)
  .trim();

const args = ["build", "-p", "aiya-audio-host"];
if (profile === "release") args.push("--release");
console.log(`[sidecar] cargo ${args.join(" ")}  (target ${triple})`);
execFileSync("cargo", args, { cwd: root, stdio: "inherit", env });

const built = join(root, "target", profile, `aiya-audio-host${exeSuffix}`);
if (!existsSync(built)) {
  console.error(`[sidecar] 构建产物不存在：${built}`);
  process.exit(1);
}

// externalBin 的约定路径
const dir = join(root, "src-tauri", "binaries");
mkdirSync(dir, { recursive: true });
const dest = join(dir, `aiya-audio-host-${triple}${exeSuffix}`);
copyFileSync(built, dest);

const mb = (statSync(dest).size / (1024 * 1024)).toFixed(2);
console.log(`[sidecar] ${dest}  (${mb} MB)`);
