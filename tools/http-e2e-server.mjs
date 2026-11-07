// http 形态 e2e 的服务端拉起器（ui/playwright.http.config.ts 的 webServer）。
//
// 职责：cargo 增量构建 aiya-server → 准备临时曲库（可播 fixtures + 一支
// AIFF 供灰标断言）与临时数据根 → 带口令启动（127.0.0.1:8799）→ 就绪探测
// → 保持到 Playwright 收尾（SIGINT/TERM 时杀子进程、清临时目录）。
//
// 数据全部落在 os.tmpdir() 下的新目录，跑完即弃；仓库与用户数据零接触。

import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const EXE = join(ROOT, 'target', 'debug', 'aiya-server.exe');
const PORT = 8799;

function log(msg) {
  console.log(`[http-e2e] ${msg}`);
}

/* ---- 1. 构建（增量；exe 已是最新时秒过） ---- */
log('ui build (vite) …');
// STATIC_DIR 托管的是构建产物——UI 源码改动必须先进 dist，否则 e2e 跑的是
// 上一轮的前端（tabs/新组件"凭空消失"就是这么来的）。
const uiBuild = spawnSync('npm', ['--prefix', join(ROOT, 'ui'), 'run', 'build'], {
  stdio: 'inherit',
  shell: process.platform === 'win32', // npm 是 .cmd，win 下 spawnSync 要经 shell
});
if (uiBuild.status !== 0) {
  console.error(`[http-e2e] ui build failed (${uiBuild.status})`);
  process.exit(1);
}

log('cargo build -p aiya-server …');
const build = spawnSync('cargo', ['build', '-p', 'aiya-server'], {
  cwd: ROOT,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (build.status !== 0) {
  console.error(`[http-e2e] cargo build failed (${build.status})`);
  process.exit(1);
}

/* ---- 2. 临时曲库 + 数据根 ---- */
const work = mkdtempSync(join(tmpdir(), 'aiya-http-e2e-'));
const musicDir = join(work, 'music');
const dataDir = join(work, 'data');
mkdirSync(musicDir, { recursive: true });
const fixtures = join(ROOT, 'crates', 'aiya-audio', 'tests', 'fixtures');
for (const f of ['tone-mp3.mp3', 'tone-vorbis.ogg', 'tone-pcm.aiff']) {
  cpSync(join(fixtures, f), join(musicDir, f));
}
log(`music=${musicDir} data=${dataDir}`);

/* ---- 3. 启动 + 就绪探测 ---- */
const child = spawn(
  EXE,
  [],
  {
    cwd: ROOT,
    env: {
      ...process.env,
      AIYA_PORT: String(PORT),
      AIYA_BIND: '127.0.0.1',
      AIYA_STATIC_DIR: join(ROOT, 'ui', 'dist'),
      AIYA_DATA_ROOT: dataDir,
      AIYA_MUSIC_DIR: musicDir,
      AIYA_ADMIN_PASSWORD: 'e2e-pass',
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  },
);
child.on('exit', (code) => {
  console.error(`[http-e2e] server exited early (${code})`);
  cleanup();
  process.exit(1);
});

const deadline = Date.now() + 30_000;
async function ready() {
  while (Date.now() < deadline) {
    if (child.exitCode !== null) process.exit(1); // 早退由 exit handler 处理
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/auth/state`);
      if (r.ok) {
        log('server ready');
        return;
      }
    } catch {
      /* 未就绪，继续等 */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  console.error('[http-e2e] server not ready in 30s');
  cleanup();
  process.exit(1);
}

/* ---- 4. 收尾 ---- */
function cleanup() {
  try {
    if (child.exitCode === null) child.kill();
  } catch {
    /* 已退出 */
  }
  try {
    rmSync(work, { recursive: true, force: true });
  } catch {
    /* 下次系统的 temp 清理兜底 */
  }
}
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(sig, () => {
    cleanup();
    process.exit(0);
  });
}
process.on('exit', cleanup);

/* 端口被占的兜底：playwright webServer 靠端口探测就绪，占用会静默复用旧
   实例——先显式探测并拒绝，宁可失败也不测到陈旧状态 */
const probe = createServer();
probe.once('error', () => {
  console.error(`[http-e2e] port ${PORT} is already in use`);
  process.exit(1);
});
probe.once('listening', () => {
  probe.close();
  void ready();
});
probe.listen(PORT, '127.0.0.1');
