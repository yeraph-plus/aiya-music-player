/**
 * 高分屏缩放诊断（docs/56）：往控制台打一行缩放链路的关键值。
 *
 * 为什么要有它：「高分屏文字过小」类反馈的第一道分诊——
 * - dpr ≈ 系统缩放（150% 屏上 1.5）→ 缩放链路正常，问题在字号设计（界面缩放
 *   档位就是给这类用户的）；
 * - dpr = 1.0 而系统明明是 150% → 缩放失效，查环境（RDP 会话 / 旧 WebView2
 *   Runtime / 跨屏拖动后未刷新）。
 *
 * `scale` / `inner` 只有 Tauri 宿主下拿得到（`scaleFactor()` / `innerSize()`，
 * 物理像素）；浏览器预览只有 dpr 与 CSS 视口，缺的部分静默跳过。
 * info 级、启动一次 + resize（跨屏拖动会触发）防抖 400ms 一次，不进任何
 * 错误哨兵的统计面。
 */
const IS_TAURI = '__TAURI_INTERNALS__' in window;

function describeViewport(): string {
  return `dpr=${window.devicePixelRatio} viewport=${window.innerWidth}x${window.innerHeight}(css)`;
}

async function report(): Promise<void> {
  if (!IS_TAURI) {
    console.info(`[dpi] ${describeViewport()}`);
    return;
  }
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    const [scale, inner] = await Promise.all([win.scaleFactor(), win.innerSize()]);
    // zoom≈dpr/scale：「禁用系统缩放」开启时 ≈ 1/scale（150% 屏上是 0.67），
    // 这行让两种"字小"在日志里可直接区分——是档位问题还是缩放被禁用了
    const zoom = (window.devicePixelRatio / scale).toFixed(2);
    // logical = 物理 / 缩放：与 tauri.conf.json 的 1280×800 对得上说明没有
    // 物理/逻辑混用（docs/56 的回归哨兵）
    console.info(
      `[dpi] ${describeViewport()} scale=${scale} zoom≈${zoom} inner=${inner.width}x${inner.height}(physical) ` +
        `logical=${Math.round(inner.width / scale)}x${Math.round(inner.height / scale)}`,
    );
  } catch {
    console.info(`[dpi] ${describeViewport()} scale=?（宿主 API 未就绪，仅 dpr 可用）`);
  }
}

/** 启动诊断并在窗口尺寸变化（含跨屏）时补报；返回清理函数（当前无人卸载，留 symmetry） */
export function startDpiDiagnostics(): () => void {
  let timer: number | undefined;
  const onResize = (): void => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void report(), 400);
  };
  window.addEventListener('resize', onResize);
  void report();
  return () => {
    window.removeEventListener('resize', onResize);
    window.clearTimeout(timer);
  };
}
