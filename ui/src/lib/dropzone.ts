/**
 * 壳级拖放（docs/52 §5.2）：把操作系统的"拖文件进窗口"接成三个回调。
 *
 * ## 为什么不用 HTML5 `ondrop`
 *
 * Tauri v2 的 `dragDropEnabled` 默认为 `true`，此时原生层**接管**了从资源
 * 管理器拖进来的文件，webview 里收不到 `dragover` / `drop` 事件（HTML5 拖放
 * 只对页面内元素之间的拖拽还生效，库内行排序靠的就是那一套，见 `lib/drag.ts`）。
 * 所以入口只能是 `onDragDropEvent`。
 *
 * ## 为什么单独一层
 *
 * 与 `lib/window-chrome.ts` 同一条理由：窗口/宿主能力不属于数据协议，
 * 浏览器预览里它根本不存在，正确降级是**什么都不做**而不是"塞个假实现"。
 * 也因此 `components/` 与 `views/` 不许直接 `import '@tauri-apps/api/*'`
 * （`npm run check:ui-layers` 守着），要用就从这里取。
 */
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { isNative } from './window-chrome';

export interface DropHandlers {
  /** 拖着文件进入窗口（payload 里带 paths，可以据此预告会发生什么） */
  onEnter: (paths: string[]) => void;
  /** 拖走了 / 取消了 / 已经落下（落下后也要收起提示） */
  onLeave: () => void;
  /** 松手：真正的落点 */
  onDrop: (paths: string[]) => void;
}

/**
 * 订阅文件拖放。返回清理函数（浏览器预览下是空操作）。
 *
 * `over` 事件按鼠标移动频率触发（几十 Hz），**不转发**：它只带 `position`，
 * 而拖放提示不需要跟随光标——转成响应式状态只会让整个壳层在拖拽期间每帧
 * 重渲染一遍。
 */
export function onFileDrop(handlers: DropHandlers): () => void {
  if (!isNative) return () => {};

  let unlisten: (() => void) | null = null;
  let disposed = false;

  void getCurrentWebview()
    .onDragDropEvent((event) => {
      const payload = event.payload;
      switch (payload.type) {
        case 'enter':
          handlers.onEnter(payload.paths);
          break;
        case 'leave':
          handlers.onLeave();
          break;
        case 'drop':
          handlers.onLeave();
          handlers.onDrop(payload.paths);
          break;
        default:
          break;
      }
    })
    .then((off) => {
      // 订阅是异步完成的：期间组件已经卸载就先退订，否则监听器泄漏在 webview 上
      if (disposed) off();
      else unlisten = off;
    })
    .catch((error: unknown) => {
      // 拖放接不上不该让应用起不来：所有功能都有别的入口（设置→曲库、
      // 直播页的地址框与「打开文件…」），这里只留一条 warn 给排查用
      console.warn('onDragDropEvent failed:', error);
    });

  return () => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  };
}
