/**
 * 窗口 chrome：标题栏三键 + 最大化状态。
 *
 * **为什么单独一层**：`lib/ipc/transport.ts` 是**数据协议**的唯一换气口
 * （命令 + 事件，浏览器预览时由 mock 整体替换实现）。窗口能力是另一种东西：
 * 预览里它根本不存在，正确处理是"按钮禁用"，不是"换成假实现"——把两者塞进
 * 同一个抽象会让 transport 背上两种完全不同的降级语义。
 *
 * 也因此本模块**不经过 mock**：`isNative` 为 false 时全是空操作。
 * 组件层（`components/`、`views/`）只许从这里取窗口能力，不许直接
 * `import '@tauri-apps/api/*'`（由 `npm run check:ui-layers` 守着）。
 */
import { getCurrentWindow } from '@tauri-apps/api/window';

type AppWindow = ReturnType<typeof getCurrentWindow>;

/** 是否跑在真宿主里（浏览器预览 = false）。按钮的禁用态也用它。 */
export const isNative = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 惰性创建且只建一次。
 *
 * 必须惰性：浏览器预览下没有 Tauri 元数据，模块顶层调 `getCurrentWindow()`
 * 会直接抛错（不是返回 null），整个 chunk 加载失败变成白屏。
 */
let cached: AppWindow | null = null;
function windowRef(): AppWindow | null {
  if (!isNative) return null;
  if (!cached) {
    try {
      cached = getCurrentWindow();
    } catch {
      cached = null;
    }
  }
  return cached;
}

export function minimizeWindow(): void {
  void windowRef()?.minimize();
}

export function toggleMaximizeWindow(): void {
  void windowRef()?.toggleMaximize();
}

export function closeWindow(): void {
  void windowRef()?.close();
}

/** 当前是否最大化。非宿主环境恒 false。 */
export async function isWindowMaximized(): Promise<boolean> {
  const win = windowRef();
  if (!win) return false;
  try {
    return await win.isMaximized();
  } catch {
    return false;
  }
}

/**
 * 订阅窗口尺寸变化（最大化状态可能被系统改：拖到屏幕边缘吸附、Win+↑）。
 * 返回**同步**的取消函数，宿主侧的 Promise 在内部收尾。
 */
export function onWindowResized(cb: () => void): () => void {
  const win = windowRef();
  if (!win) return () => {};
  let stopped = false;
  const pending = win.onResized(() => cb());
  return () => {
    if (stopped) return;
    stopped = true;
    void pending.then((stop) => stop()).catch(() => {});
  };
}
