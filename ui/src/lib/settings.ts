/**
 * 设置项的类型与默认值。
 *
 * 2026-10-25 批次 4：从已删除的 `@aiya/contracts` 搬来。
 *
 * 与 `lib/ipc/schema.ts` 的分工（两者**刻意并存**，别合并）：
 * - 这里：**写代码时**要的类型与默认值（字段必填、有初始值）；
 * - `schema.ts`：**读磁盘时**的 zod 校验（字段全 optional + passthrough，
 *   因为老配置文件缺字段是常态）。
 *
 * 已知偏差：磁盘上有 `output`（输出模式/缓冲/时钟，2026-10 加的），
 * 但这套类型还没跟上（`mergeSettings` 靠 passthrough 把它原样带进来，
 * 类型上看不见）。设置页目前不编辑 `output`，所以没有实际影响——
 * 接设置页的"输出"分区时要把字段补进 `AppSettings`。
 */

import type { ShortcutAction } from './shortcuts';

export type ThemeMode = 'dark' | 'light';
export type ListDensity = 'comfortable' | 'compact';

export interface AppSettings {
  theme: ThemeMode;
  density: ListDensity;
  /** 从封面提取主色作为强调色；关闭则用固定主题色 */
  accentFromArtwork: boolean;
  /** 按 ReplayGain 调整音量（近似实现：换算到音量系数） */
  normalizeVolume: boolean;
  /**
   * 频谱可视化。
   *
   * 开启时渲染层会订阅引擎 tap（`spectrum_set(true)`）；关闭时
   * 面板按钮整个不出现，引擎侧零开销。
   */
  spectrum: boolean;
  /**
   * 频谱面板渲染引擎。
   *
   * compact：自研轻量条形（零依赖）；
   * pro：audioMotion-analyzer 专业渲染（径向/LED/镜像等，AGPL-3.0，见 docs/09）。
   */
  spectrumEngine: 'compact' | 'pro';
  /**
   * 系统媒体控制（Windows SMTC / 锁屏 / 蓝牙耳机）。
   *
   * 开启后由系统路由媒体键 —— 这也是为什么不再注册全局快捷键：
   * `globalShortcut` 一旦占用媒体键，Chromium 就不再向系统发布媒体会话，
   * 系统媒体面板会整个消失（实测）。
   */
  mediaKeys: boolean;
  /** 应用启动后自动播放上次的曲目 */
  autoplayOnStart: boolean;
  /** 记住上次播放进度；关闭则每次都从头开始 */
  rememberPosition: boolean;
  /** 播放时阻止系统休眠（允许熄屏） */
  preventSleep: boolean;
  /** 关闭窗口时最小化到托盘而不是退出 */
  closeToTray: boolean;
  /** 只存覆盖项：没写的动作走 `DEFAULT_SHORTCUTS`（改默认值不冲用户自定义） */
  shortcuts: Partial<Record<ShortcutAction, string>>;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  density: 'comfortable',
  accentFromArtwork: true,
  normalizeVolume: false,
  // 频谱默认关：不订阅 tap，保持最简出口路径
  spectrum: false,
  // 默认专业引擎：装饰观感优先；想要零依赖轻量路径可切回 compact
  spectrumEngine: 'pro',
  mediaKeys: true,
  // 启动就出声容易吓到人，默认关闭；想开的人在设置里开
  autoplayOnStart: false,
  // 进度记忆默认开：这是播放器的通行行为
  rememberPosition: true,
  // 默认开：否则整晚放歌会让电脑睡不着
  preventSleep: true,
  // 默认关：关窗即退出是既有行为，不制造意外
  closeToTray: false,
  shortcuts: {},
};
