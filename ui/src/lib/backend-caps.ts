/**
 * 后端能力面（docs/66 §10）：桌面宿主（Tauri 壳 + Rust 引擎）与 Web 宿主
 * （aiya-server + 浏览器 `<audio>`）共用同一套 UI，但一部分功能只存在于
 * 桌面侧——EQ 与输出设备是独立进程音频引擎的 DSP（ADR-0002），托盘、
 * 防休眠、webview 缩放补偿是窗口壳的能力。Web 下这些入口不渲染，避免
 * 给出"调了没反应"的死开关；设置字段本身照常落库（DB 不做客户端分支，
 * docs/66 §8.2），桌面侧语义不受影响。
 *
 * 判据与 boot 三态同源：`__AIYA_WEB__` 仅由 http 传输装载时写入
 * （ipc/http.ts），且装载先于 mount（main.ts）。在组件初始化期调用即可靠，
 * 不要在模块顶层取值（模块求值可能早于传输装载）。tauri 与 mock（e2e、
 * 浏览器预览）恒为桌面能力——e2e 对桌面形态的断言不受影响。
 */

/** 各宿主形态的能力差。true = 该功能在本宿主可用（渲染其入口） */
export interface BackendCaps {
  /** EQ：Rust 引擎 DSP；浏览器 `<audio>` 管线无此段（WebAudio EQ 属后续可选项） */
  eq: boolean;
  /** 输出设备（共享/独占、缓冲、时钟、暂停释放）：WASAPI 专属 */
  output: boolean;
  /** 播放时阻止休眠：桌面电源集成（SetThreadExecutionState） */
  preventSleep: boolean;
  /** 窗口壳（关闭到托盘、禁用系统缩放）：浏览器没有窗口壳语义 */
  shell: boolean;
  /** 系统目录/文件选择器（pick_folder 系命令）：窗口壳能力，浏览器没有 */
  pickFolder: boolean;
}

export function backendCaps(): BackendCaps {
  const web =
    typeof window !== 'undefined' &&
    !!(window as { __AIYA_WEB__?: unknown }).__AIYA_WEB__;
  return { eq: !web, output: !web, preventSleep: !web, shell: !web, pickFolder: !web };
}
