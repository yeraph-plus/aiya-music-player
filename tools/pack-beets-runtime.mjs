// 生成随包的 beets 运行时（docs/29 批 C）。
//
// 产物：tools/beets-runtime/  （embeddable Python + site-packages，约 110MB）
//   python.exe            自带解释器与标准库
//   python314._pth        把 site-packages 加进 sys.path
//   site-packages/        beets 及其全部依赖（按 tools/beets-requirements.lock）
//
// 为什么是这个形态而不是 venv：venv **不能随包分发**。pyvenv.cfg 的 `home`
// 指向建库时的系统 Python，标准库和 python314.dll 都在那儿；拷到没有同版本
// Python 的机器上，python.exe 会直接报 did not find executable（实测，见
// docs/29 §15.1）。embeddable 发行版自带这些，没有外部依赖。
//
// 用法：
//   node tools/pack-beets-runtime.mjs            # 首次构建/增量
//   node tools/pack-beets-runtime.mjs --force    # 删掉重建
//   node tools/pack-beets-runtime.mjs --offline  # 只用缓存，不联网
//
// 为什么要给 embeddable 自己装 pip：pip install 出来的 wheel 是按**运行 pip 的
// 那个解释器**的 ABI 选的（cp314）。用系统 Python 的 pip 装进 embeddable 的
// site-packages，版本一不一致就会装进 cp312 的 .pyd 而运行时 import 失败。
// 所以这里先 get-pip，再用 embeddable 自己装。

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PY_VERSION = '3.14.7';
// _pth 文件名只带主次版本：python314._pth（3.14.x 都用它）。
// 写成全版本号 python3147._pth 的话解释器根本不会读，症状是"package 装了但 import 不到"。
const PY_TAG = PY_VERSION.split('.').slice(0, 2).join('');
const EMBED_URL = `https://www.python.org/ftp/python/${PY_VERSION}/python-${PY_VERSION}-embed-amd64.zip`;
const GET_PIP_URL = 'https://bootstrap.pypa.io/get-pip.py';

const RUNTIME_DIR = path.join(ROOT, 'tools', 'beets-runtime');
// 跟 pip 的默认落点保持一致（get-pip / pip install 都装到 Lib/site-packages）。
// 自己另选一个目录名就得在每处 pip 调用都加 --target，多一处漏就多一种"装了但用不到"。
const SITE_PACKAGES = path.join(RUNTIME_DIR, 'Lib', 'site-packages');
const CACHE_DIR = path.join(ROOT, 'tools', '.cache');
const LOCK_FILE = path.join(ROOT, 'tools', 'beets-requirements.lock');

const argv = new Set(process.argv.slice(2));
const FORCE = argv.has('--force');
const OFFLINE = argv.has('--offline');

function log(msg) {
  process.stdout.write(`${msg}\n`);
}

function fail(msg) {
  process.stderr.write(`\n✗ ${msg}\n`);
  process.exit(1);
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: ROOT, ...opts });
  if (r.status !== 0) fail(`${cmd} ${args.join(' ')} 失败（退出码 ${r.status}）`);
}

async function download(url, dest) {
  if (fs.existsSync(dest) && !FORCE) {
    log(`  缓存命中 ${path.relative(ROOT, dest)}`);
    return dest;
  }
  if (OFFLINE) fail(`--offline 但缓存缺失：${path.relative(ROOT, dest)}`);
  log(`  下载 ${url}`);
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) fail(`下载失败 ${res.status} ${res.statusText}：${url}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const declared = Number(res.headers.get('content-length') || 0);
  if (declared && buf.length !== declared) {
    // 截断的 zip 会在解压阶段以看不懂的错误爆掉，这里先拦住
    fail(`下载不完整：声明 ${declared} 字节，实收 ${buf.length}`);
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(dest, buf);
  const sha = createHash('sha256').update(buf).digest('hex');
  log(`  完成 ${(buf.length / 1024 / 1024).toFixed(1)} MB  sha256=${sha.slice(0, 16)}…`);
  return dest;
}

function dirSize(dir) {
  let total = 0;
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.isFile()) total += fs.statSync(p).size;
    }
  };
  if (fs.existsSync(dir)) walk(dir);
  return total;
}

/**
 * 解 zip。
 *
 * 为什么不直接用 `tar -xf`：PATH 上的 GNU tar 会把 `D:\...` 当成 rsh 的
 * `host:path`，报 "Cannot connect to D: resolve failed"（实测）。PowerShell 的
 * Expand-Archive 是 Windows 原生、路径处理正确，作为首选；tar 加 --force-local
 * 作为回退（bsdtar 不认这个参数，所以两条路互为补充而不是串联）。
 */
function unzip(zip, dest) {
  const ps = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', `Expand-Archive -LiteralPath "${zip}" -DestinationPath "${dest}" -Force`],
    { stdio: 'inherit' },
  );
  if (ps.status === 0) return;
  log('  Expand-Archive 失败，回退 tar --force-local');
  const t = spawnSync('tar', ['--force-local', '-xf', zip, '-C', dest], { stdio: 'inherit' });
  if (t.status !== 0) fail('解压失败：Expand-Archive 与 tar 都没成功');
}

async function main() {
  log('== 生成 beets 运行时（embeddable Python + site-packages）==');
  if (!fs.existsSync(LOCK_FILE)) fail(`缺少依赖锁定文件：${path.relative(ROOT, LOCK_FILE)}`);

  if (FORCE && fs.existsSync(RUNTIME_DIR)) {
    log(`删除旧运行时 ${path.relative(ROOT, RUNTIME_DIR)}`);
    fs.rmSync(RUNTIME_DIR, { recursive: true, force: true });
  }
  fs.mkdirSync(RUNTIME_DIR, { recursive: true });
  fs.mkdirSync(CACHE_DIR, { recursive: true });

  const pyExe = path.join(RUNTIME_DIR, 'python.exe');
  if (!fs.existsSync(pyExe)) {
    // 1) embeddable 运行时
    const zip = await download(EMBED_URL, path.join(CACHE_DIR, `python-${PY_VERSION}-embed-amd64.zip`));
    log('  解压 embeddable');
    unzip(zip, RUNTIME_DIR);
  } else {
    log('  解释器已存在，跳过解压');
  }

  // 2) _pth：把 site-packages 加进 sys.path，并启用 site
  //    （默认 _pth 会把 site.main() 关掉，关了之后 site-packages 不会被扫）
  //
  //    这一行**每次都要写**，不能放进上面的"解压"分支：解压出来的原始包自带一份
  //    默认 _pth，只在首次写的话，中途失败重跑就把自己修过的内容丢了 ——
  //    症状是"pip 明明装成功，import 还是 ModuleNotFoundError"。
  fs.writeFileSync(
    path.join(RUNTIME_DIR, `python${PY_TAG}._pth`),
    ['python314.zip', '.', 'Lib\\site-packages', '', 'import site', ''].join('\r\n'),
    'utf8',
  );

  // 3) 给 embeddable 装 pip（首次）
  const pipMarker = path.join(SITE_PACKAGES, 'pip');
  if (!fs.existsSync(pipMarker)) {
    fs.mkdirSync(SITE_PACKAGES, { recursive: true });
    const getPip = await download(GET_PIP_URL, path.join(CACHE_DIR, 'get-pip.py'));
    log('  安装 pip（用 embeddable 自己，保证 wheel 的 ABI 匹配）');
    run(pyExe, [getPip, '--no-warn-script-location', '--disable-pip-version-check']);
  } else {
    log('  pip 已存在，跳过');
  }

  // 4) 装 beets 依赖（--upgrade 让重复执行等价于升到 lock 的版本）
  //
  // 锁文件内容没变就跳过：`pip install --upgrade` 每次都要连 PyPI 核对
  // （十几秒起），而 `npm run bundle` 会顺手带上这一步 —— 每次打包都等一遍
  // 是不可接受的。用 lock 的 sha256 当指纹，改了 lock 自动重装。
  const lockSha = createHash('sha256').update(fs.readFileSync(LOCK_FILE)).digest('hex');
  const stampFile = path.join(RUNTIME_DIR, '.lock-sha256');
  const installed = fs.existsSync(path.join(SITE_PACKAGES, 'beets'));
  if (installed && fs.existsSync(stampFile) && fs.readFileSync(stampFile, 'utf8').trim() === lockSha) {
    log('  依赖与 lock 一致，跳过 pip install');
  } else {
    log('  pip install -r tools/beets-requirements.lock');
    run(pyExe, [
      '-m',
      'pip',
      'install',
      '--disable-pip-version-check',
      '--no-warn-script-location',
      '--upgrade',
      '-r',
      LOCK_FILE,
      '--target',
      SITE_PACKAGES,
    ]);
    fs.writeFileSync(stampFile, `${lockSha}\n`, 'utf8');
  }

  // 5) 自检：不是看退出码，是真 import 一次
  log('  自检 import beets');
  const check = spawnSync(pyExe, ['-c', 'import beets, numpy; print(beets.__version__, numpy.__version__)'], {
    encoding: 'utf8',
    env: { ...process.env, PYTHONIOENCODING: 'utf-8', PYTHONUTF8: '1' },
  });
  if (check.status !== 0) fail(`自检失败：${(check.stderr || '').slice(0, 800)}`);

  const mb = dirSize(RUNTIME_DIR) / 1024 / 1024;
  log('');
  log(`✓ 运行时就绪：${path.relative(ROOT, RUNTIME_DIR)}（${mb.toFixed(1)} MB）`);
  log(`  beets / numpy = ${check.stdout.trim()}`);
  log('  打进安装包：tauri.conf.json 的 bundle.resources 已指向该目录');
}

main().catch((e) => fail(e?.stack || String(e)));
