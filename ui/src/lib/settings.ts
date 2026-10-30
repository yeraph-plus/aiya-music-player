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

/**
 * 主题颜色：固定 CSS 渐变 + 配套强调色，成套切换。
 *
 * 每个方案在 tokens.css 按 `html[data-ambient]` 定义各自的 `--ambient-bg`
 * 与强调色三件套；`aurora` 是默认（主题自带蓝色 + 蓝青渐变）。
 * 不跟随封面 —— 早期"封面预模糊 + 主色提取"的实现已移除，代码里不再有分支。
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
 * 频谱风格（2026-10-26 起）：**正在播放页**可视化的形态（docs/26 §3）。
 *
 * 取代旧 `spectrumEngine`（compact/pro）——它的唯一消费方（SpectrumPanel
 * 五形态）已随 T4a 页面改版移除，且 pro 需要渲染层 AudioNode、原生引擎
 * 架构（ADR-0006）下本就无法成立。
 *
 * 2026-10-30（docs/46 §6）：`'off'` 退役——它和 `npSpectrum=false` 是同一个
 * 语义的第二个入口（历史叠加），现在由开关独占。旧配置里的 `'off'` 在
 * `mergeSettings` 落回默认形态，不写回。
 *
 * 同日：`'wave'`（心电图）退役。它的"每带一个 R 波 + S 波"折线在低频段
 * 会挤成一片尖刺、在高频段又拉成锯齿，看上去像算错了幅度；席位由
 * `'ripple'` 接管。旧值 `'wave'` 在迁移里映射到 `'ripple'`。
 *
 * 同日定版：`'ripple'` 在页内只画**一条居中曲线**。它中途试过地形版
 * （22 层等深线铺满下半屏），但那套和背景层 `bgSpectrumStyle='ripple'`
 * 重复，页内又只覆盖内容区、水面会在画布边缘断掉，所以地形只留给背景，
 * 页内降为单线主视觉。
 */
export type SpectrumStyle = 'mirror' | 'radial' | 'ripple';

/** 设置页与 NowPlayingSpectrum 共用的形态清单（id 顺序即展示顺序） */
export const SPECTRUM_STYLES: Array<{ value: SpectrumStyle; label: string }> = [
  { value: 'mirror', label: '镜像曲线' },
  { value: 'radial', label: '径向环形' },
  { value: 'ripple', label: '中轴波纹' },
];

/**
 * 背景频谱**光效**档位（docs/46 §3.2）。
 *
 * 这是**审美**开关，不是性能开关：三档只改 shader 里的衰减/线宽系数，
 * 成本差异可忽略。放出来是因为"柔雾（形状不可辨光）"与"锐光（脊线/线清晰）"
 * 是互斥的观感，一次性拍板风险大，让用户自己定调更稳。
 *
 * 2026-10-30 起**山形与波纹共用**这一档（用户要求"两者都应用背景频谱光效"）：
 * 两个图层的几何不同，但档位的语义统一为"光的多少 + 边缘的锐度"，否则同一个
 * 档位在两种形态下的表现不一致，会被当成开关失效。键名 `ridgeGlow` 是历史
 * 遗留（先有山形），不改名以免动迁移。
 */
export type RidgeGlow = 'soft' | 'balanced' | 'neon';

export const RIDGE_GLOWS: Array<{ value: RidgeGlow; label: string }> = [
  { value: 'soft', label: '柔雾' },
  { value: 'balanced', label: '平衡' },
  { value: 'neon', label: '锐光' },
];

/**
 * 背景频谱的形态（2026-10-30，docs/46 P5 之二）。
 *
 * 与正在播放页的 `spectrumStyle` **刻意分开**：背景是常驻氛围，页内是
 * 一次性聚焦的视觉，两者最优形态不同（背景要淡、要大、要能一直看；页内
 * 可以更亮更密）。共用一个键会让用户为了页内试形态而改掉背景。
 *
 * - `ridge` 山形：闭合填充的环境光，边界柔和，最不抢内容（默认，保持
 *   用户当前看到的样子）
 * - `ripple` 波纹：一层层等深线铺向下半屏，结构感强，适合当"水面"看
 */
export type BgSpectrumStyle = 'ridge' | 'ripple';

export const BG_SPECTRUM_STYLES: Array<{ value: BgSpectrumStyle; label: string }> = [
  { value: 'ridge', label: '山形' },
  { value: 'ripple', label: '波纹' },
];

export interface AppSettings {
  theme: ThemeMode;
  /** 主题颜色：固定渐变背景 + 配套强调色，成套切换（切换即换视觉身份） */
  ambient: AmbientScheme;
  /** 按 ReplayGain 调整音量（近似实现：换算到音量系数） */
  normalizeVolume: boolean;
  /**
   * 全局 3D / WebGL 效果总闸（2026-10-30，docs/46 §6.2）。
   *
   * **能力闸门，不是收藏夹**：关掉时所有 WebGL 图层不渲染、不创建上下文，
   * 背景退回静态 CSS 渐变；`bgSpectrum` / `npSpectrum` 的值保留但一律无效。
   * 毛玻璃（`backdrop-filter`）是 CSS 合成，不归这个闸门管。
   *
   * 为什么需要它：WebGL 在软件光栅（远程桌面 / 禁用硬件加速 / 老核显）下
   * 不可用，而这类机器没法在编译期识别。给用户一个能自己关的总闸，比
   * 只靠自动探测更可靠（探测也会错）。
   */
  glEffects: boolean;
  /**
   * 背景动态频谱（山形环境光）。
   *
   * 开启时背景层订阅引擎 tap（`spectrum_set(true)`）；关闭时只剩第 1 层
   * 静态渐变，引擎侧零开销。它同时也是毛玻璃的成本开关：背景一静止，
   * `backdrop-filter` 的结果就被合成器缓存，玻璃变一次性开销。
   */
  bgSpectrum: boolean;
  /** 正在播放页频谱。与 `bgSpectrum` 各自独立（docs/46 §5：两个页面分开控制） */
  npSpectrum: boolean;
  /** 频谱形态：正在播放页可视化的形态（背景形态见 `bgSpectrumStyle`） */
  spectrumStyle: SpectrumStyle;
  /** 背景频谱形态（山形 / 波纹）；与页内的 `spectrumStyle` 各自独立 */
  bgSpectrumStyle: BgSpectrumStyle;
  /** 背景山形光晕档位（审美开关，见 `RidgeGlow`；只对山形形态有效） */
  ridgeGlow: RidgeGlow;
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
  /**
   * 播放记录保留周期（天）。0 = 永久。
   *
   * 2026-10-28 拍板：最近播放的记录范围收敛到这一个设置——history 表
   * 只存周期内的行（宿主在启动 / 每次写历史 / 改设置时裁剪），防止
   * 重度使用一年堆几十万行。「最近播放」tab、常听榜都基于裁剪后的
   * history，自动遵循；筛选器不再另设"上次播放"档（功能重叠）。
   */
  historyDays: number;
  /** 只存覆盖项：没写的动作走 `DEFAULT_SHORTCUTS`（改默认值不冲用户自定义） */
  shortcuts: Partial<Record<ShortcutAction, string>>;
  /**
   * 输出偏好（"我想要什么"）。设备实际给了什么在诊断页的 `audioMode` 里，
   * 两者刻意分开：请求独占而设备拒绝时，这里仍是 `exclusive`。
   */
  output: DesiredOutput;
  /**
   * 直播页「最近打开」（docs/52 §7.3）。上限 20，按 URL 去重、最新在前。
   *
   * 为什么放设置 KV 而不是建表：20 条记录 + 一张表 + 增删查命令 + IPC 类型，
   * 换来的只是"记得上次开过哪几个台"。KV 已经存在、已经落盘、已经有迁移，
   * 这个量级不值得再开一条数据链。
   */
  liveRecent: LiveRecentEntry[];
}

/** 直播页最近打开的一条 */
export interface LiveRecentEntry {
  url: string;
  /** 用户手改过的标题；缺省由宿主按 URL 推（`default_stream_title`） */
  title?: string;
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  ambient: 'aurora',
  normalizeVolume: false,
  // 总闸默认开：关掉等于把这次重做的效果全废掉，不是"更省"的同义词。
  // 真不支持的机器由 capability 探测兜底（探测失败照样能正常用）
  glEffects: true,
  // 背景/页面频谱默认关：不订阅 tap，保持最简出口路径
  bgSpectrum: false,
  npSpectrum: false,
  // 默认镜像曲线：T4h 用户拍板的首发形态
  spectrumStyle: 'mirror',
  // 背景默认仍是山形：它是长期存在的那一层，不因为新增形态就替用户换掉
  bgSpectrumStyle: 'ridge',
  // 平衡档：柔雾（无光）与锐光之间的中点，有一点光效但不刺眼
  ridgeGlow: 'balanced',
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
  // 90 天：轻量用户一年也不会堆到哪儿去，重度用户也有明确的"还在听"语义
  historyDays: 90,
  shortcuts: {},
  // 共享 + 200ms + 事件驱动：最不容易出事的组合。独占要用户主动开，
  // 因为它会锁住设备（别的程序放不出声）。
  output: { mode: 'shared', bufferMs: 200, clock: 'auto', releaseOnPause: true },
  // 空表起步：没开过直播的机器不该看到一条假记录
  liveRecent: [],
};
