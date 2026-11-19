// 第一行，必须在任何会构造 zod schema 的模块之前（原因见该文件注释）
import './lib/zod-setup';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { applyLook, readLook } from './lib/look';
import { startDpiDiagnostics } from './lib/dpi-diagnostics';
import type { LoginAuth } from './lib/login';
import type { Look } from './lib/look';

let previewLogin: LoginAuth | null = null;
let previewLook: Look | null = null;

/**
 * 浏览器预览（ui/ 目录下 `npm run dev`）的装配点：没有 Tauri 运行时就把假后端
 * 装进 `lib/ipc/transport`，让所有 `ipc.*` 与事件订阅照常工作。
 *
 * 用动态 import 而不是顶层静态引入：mock 在 Tauri 构建里是独立 chunk，
 * 真宿主永不加载，也不会把 20KB 假数据塞进主 bundle。
 *
 * `?failCmd=library_list` 可注入一条命令失败，用来手验错误条渲染。
 * `?beets=absent` 让 `beets_status` 报"未安装"，用来演练 Beets 未安装的收拢形态
 * （e2e 用它守这条路径）。
 * `?missing=2` 让 `library_stats` 报出位置缺失，用来演练首页那条告警 →
 * 设置页曲库组的跳转（e2e 用它证明"跳过去确实落在了曲库组"）。
 */
if (!('__TAURI_INTERNALS__' in window)) {
  const params = new URLSearchParams(window.location.search);
  const mode = params.get('backend');
  // 宿主三态：tauri（默认）/ http（server 注入的 __AIYA_BOOT__ 标志，或
  // ?backend=http）/ mock（预览与 e2e；?backend=mock 强制）。无标志不探测——
  // 探测的 404 会在 e2e 的运行期错误收集器里记账。
  const useHttp = mode !== 'mock' && (mode === 'http' || (window as { __AIYA_BOOT__?: string }).__AIYA_BOOT__ === 'http');
  if (useHttp) {
    const { installHttpTransport, probeAuthGate } = await import('./lib/ipc/http');
    installHttpTransport();
    // 进闸预检必须先于 mount：登录卡首帧即渲染，主界面不再闪一帧
    await probeAuthGate();
  } else {
    const { installMock } = await import('./lib/ipc/mock');
    const preview = import.meta.env.DEV ? await import('./dev/preview') : null;
    const previewConfig = preview?.previewOptions(params);
    if (previewConfig?.seed?.settings) {
      const settings = previewConfig.seed.settings;
      previewLook = { theme: settings.theme!, ambient: 'aurora', uiScale: settings.uiScale! };
      const { setLocale } = await import('./lib/i18n.svelte');
      setLocale(settings.language === 'en' ? 'en' : 'zh');
    }
    if (import.meta.env.DEV && params.has('gate')) {
      const { previewAuth } = await import('./dev/login');
      previewLogin = previewAuth(params);
    }
    const failCmd = params.get('failCmd');
    const missing = Number(params.get('missing'));
    const live = params.get('live');
    const uninstallMock = installMock({
      ...previewConfig,
      ...(failCmd ? { failCommands: [failCmd] } : {}),
      ...(params.get('beets') === 'absent' ? { beetsFound: false } : {}),
      ...(Number.isFinite(missing) && missing > 0 ? { missingLocations: missing } : {}),
      // `?live=<url>`：启动即进"正在播一个直播流"，用来验播放条的直播三态
      // 这条渲染分支没有别的入口能稳定复现
      ...(live ? { liveStream: live } : {}),
    });
    import.meta.hot?.dispose(uninstallMock);
  }
}

const target = document.getElementById('app');
if (!target) throw new Error('#app not found');

/**
 * 首帧外观：**必须早于 mount**。
 *
 * 主题真值在 SQLite，`bootstrap()` 之后才到；这里读的是 `App` 主题 effect
 * 上次写下的 localStorage 镜像（`lib/look.ts` 有完整理由）。三级回退：
 * 镜像 → `__AIYA_LOOK__` → 默认深色。
 */
applyLook(previewLook ?? readLook());
// 高分屏分诊日志：dpr 是否等于系统缩放，一眼定位"文字过小"的类别
startDpiDiagnostics();

const app = previewLogin
  ? mount((await import('./components/LoginView.svelte')).default, { target, props: { auth: previewLogin } })
  : mount(App, { target });

export default app;
