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
 * `output`（2026-10-25 补）：输出模式/缓冲/时钟。形状直接用 Rust 生成的
 * `DesiredOutput` —— 枚举取值（`shared` / `exclusive` / `exclusiveRequired` /
 * `null`、`auto` / `events` / `polling`）由 `aiya-audio` 的 `parse` 定义，
 * 前端抄一份字符串常量迟早对不上。
 */

import type { DesiredOutput } from './ipc/generated/DesiredOutput';
import type { ShortcutAction } from './shortcuts';

export type ThemeMode = 'dark' | 'light';
export type ListDensity = 'comfortable' | 'compact';

/**
 * 背景方案（docs/25 §5 T4d）：固定 CSS 渐变 + 配套强调色，成套切换。
 *
 * 取代「封面预模糊背景 + 封面主色提取」（用户拍板移除）：
 * 背景不再跟随曲目，而是用户选定的固定视觉身份。`aurora` 就是原默认
 * （主题自带蓝色强调 + 蓝青渐变），其余方案在 tokens.css 按
 * `html[data-ambient]` 定义各自的 `--ambient-bg` 与强调色三件套。
 */
export type AmbientScheme = 'aurora' | 'ember' | 'meadow' | 'dusk' | 'mono';

/** 设置页与 look 校验共用的方案清单（id 顺序即展示顺序） */
export const AMBIENT_SCHEMES: Array<{ value: AmbientScheme; label: string }> = [
  { value: 'aurora', label: '极光' },
  { value: 'ember', label: '流霞' },
  { value: 'meadow', label: '苔原' },
  { value: 'dusk', label: '暮紫' },
  { value: 'mono', label: '素白' },
];

/**
 * 频谱风格（2026-10-26 起）：**正在播放页**居中可视化的形态（docs/26 §3）。
 *
 * 取代旧 `spectrumEngine`（compact/pro）——它的唯一消费方（SpectrumPanel
 * 五形态）已随 T4a 页面改版移除，且 pro 需要渲染层 AudioNode、原生引擎
 * 架构（ADR-0006）下本就无法成立。背景环境光固定为山形，不受此项影响；
 * `off` = 正在播放页不画居中可视化（只剩背景环境光）。
 */
export type SpectrumStyle = 'mirror' | 'radial' | 'wave' | 'off';

/** 设置页与 NowPlayingSpectrum 共用的形态清单（id 顺序即展示顺序） */
export const SPECTRUM_STYLES: Array<{ value: SpectrumStyle; label: string }> = [
  { value: 'mirror', label: '镜像曲线' },
  { value: 'radial', label: '径向环形' },
  { value: 'wave', label: '心电图' },
  { value: 'off', label: '关闭' },
];

export interface AppSettings {
  theme: ThemeMode;
  density: ListDensity;
  /** 背景与强调色方案（固定渐变，不跟随封面） */
  ambient: AmbientScheme;
  /** 按 ReplayGain 调整音量（近似实现：换算到音量系数） */
  normalizeVolume: boolean;
  /**
   * 频谱可视化。
   *
   * 开启时渲染层会订阅引擎 tap（`spectrum_set(true)`）；关闭时
   * 面板按钮整个不出现，引擎侧零开销。
   */
  spectrum: boolean;
  /** 频谱风格：正在播放页居中可视化的形态（背景环境光固定山形，不受影响） */
  spectrumStyle: SpectrumStyle;
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
  /**
   * 自动刷新曲库的间隔（秒）。0 = 只保留"切回窗口时刷新"。
   *
   * 为什么让它可配而不是写死 30 s：库大的机器上一次刷新要扫几万条目录，
   * 有人会想调慢；反过来改标签频繁的人想调快。下限由宿主夹到 10 s
   * （再短就该上文件监听了，那是另一套东西）。
   */
  refreshSeconds: number;
  /** 只存覆盖项：没写的动作走 `DEFAULT_SHORTCUTS`（改默认值不冲用户自定义） */
  shortcuts: Partial<Record<ShortcutAction, string>>;
  /**
   * 输出偏好（"我想要什么"）。设备实际给了什么在诊断页的 `audioMode` 里，
   * 两者刻意分开：请求独占而设备拒绝时，这里仍是 `exclusive`。
   */
  output: DesiredOutput;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  density: 'comfortable',
  ambient: 'aurora',
  normalizeVolume: false,
  // 频谱默认关：不订阅 tap，保持最简出口路径
  spectrum: false,
  // 默认镜像曲线：T4h 用户拍板的首发形态
  spectrumStyle: 'mirror',
  mediaKeys: true,
  // 启动就出声容易吓到人，默认关闭；想开的人在设置里开
  autoplayOnStart: false,
  // 进度记忆默认开：这是播放器的通行行为
  rememberPosition: true,
  // 默认开：否则整晚放歌会让电脑睡不着
  preventSleep: true,
  // 默认关：关窗即退出是既有行为，不制造意外
  closeToTray: false,
  // 30 秒：外部改标签/搬文件能在半分钟内体现，而一次增量扫描只有几十毫秒
  refreshSeconds: 30,
  shortcuts: {},
  // 共享 + 200ms + 事件驱动：最不容易出事的组合。独占要用户主动开，
  // 因为它会锁住设备（别的程序放不出声）。
  output: { mode: 'shared', bufferMs: 200, clock: 'auto', releaseOnPause: true },
};
