/**
 * 设置项类型与默认值。
 *
 * 与 `lib/ipc/schema.ts` 分工并存，不合并：
 * - 本文件：编译期类型与默认值，字段必填且有初始值；
 * - `schema.ts`：读磁盘时的 zod 校验，字段全 optional + passthrough——旧配置
 *   文件缺字段是常态。
 *
 * `output` 的形状复用 Rust 的 `DesiredOutput`：取值集合（`shared` / `exclusive` /
 * `exclusiveRequired` / `null`，`auto` / `events` / `polling`）由 `aiya-audio` 的
 * `parse` 定义，前端另存字符串常量会与后端失配。
 */

import type { DesiredOutput } from './ipc/generated/DesiredOutput';
import type { ShortcutAction } from './shortcuts';
import type { Msg } from './i18n.svelte';

/**
 * 主题偏好。`'system'` 表示跟随操作系统的深浅色（`prefers-color-scheme`）。
 *
 * 该值是**偏好**而非实际主题：`<html data-theme>` 上写的是解析后的
 * `'dark'` / `'light'`（CSS 只有这两套变量），解析规则见 `lib/look.ts` 的
 * `resolveTheme`。
 */
export type ThemeMode = 'dark' | 'light' | 'system';

/** 主题偏好的取值清单：设置页分段控件与 `mergeSettings` 白名单共用，顺序即展示顺序。
 *  `label` 存 i18n 键（settings 域），渲染点（SettingsPanel）经 `t()` 取文案 */
export const THEME_MODES: Array<{ value: ThemeMode; label: Msg }> = [
  { value: 'dark', label: 'settings.themeMode.dark' },
  { value: 'light', label: 'settings.themeMode.light' },
  { value: 'system', label: 'settings.themeMode.system' },
];

/**
 * 主题配色：固定 CSS 渐变 + 配套强调色，成套切换。
 *
 * 每个方案在 tokens.css 以 `html[data-ambient]` 定义 `--ambient-bg` 与强调色
 * 三件套；`aurora` 为默认。配色不取自封面：主色提取方案已移除，无对应分支。
 */
export type AmbientScheme = 'aurora' | 'ember' | 'meadow' | 'dusk' | 'mono';

/** 设置页与 look 校验共用的方案清单（id 顺序即展示顺序）；label 同 THEME_MODES 存 i18n 键 */
export const AMBIENT_SCHEMES: Array<{ value: AmbientScheme; label: Msg }> = [
  { value: 'aurora', label: 'settings.ambient.aurora' },
  { value: 'ember', label: 'settings.ambient.ember' },
  { value: 'meadow', label: 'settings.ambient.meadow' },
  { value: 'dusk', label: 'settings.ambient.dusk' },
  { value: 'mono', label: 'settings.ambient.mono' },
];

/**
 * 界面缩放档位：整体缩放文字与间距。
 *
 * 实现为 `html` 内联 font-size（1rem = 16px × 档位），tokens.css 的尺寸轴以 rem
 * 计，因此切换档位是全局整倍缩放，相对比例不变。与行高类密度档不同：后者改行高
 * 不改字号。档位外的值（手改配置出现 87.5 等）收敛到 100。
 */
export const UI_SCALES = [90, 100, 110, 125, 150] as const;
export type UiScale = (typeof UI_SCALES)[number];

/** 设置页分段控件直接消费的选项（value 是 string，回调里 normalizeUiScale 收口） */
export const UI_SCALE_OPTIONS: Array<{ value: string; label: string }> = UI_SCALES.map(
  (scale) => ({ value: String(scale), label: `${scale}%` }),
);

export function normalizeUiScale(value: unknown): UiScale {
  const n = typeof value === 'number' ? value : Number(value);
  const found = (UI_SCALES as ReadonlyArray<number>).find((scale) => scale === n);
  // UI_SCALES 以 100 起兜底；find 命中时 n 就是档位之一
  return (found ?? 100) as UiScale;
}

/**
 * 正在播放页的频谱形态。
 *
 * 取值：`mirror` 镜像、`radial` 环带、`ripple` 残影（中轴单线 + 反馈式残影
 * 缓冲）、`wave` 波纹（频谱曲线逐带交替成山与谷，振幅跨中线上下两段）。
 * `ripple` 在页内只画一条居中曲线：等深线地形形态（多层等深线铺满下半屏）
 * 与背景层 `bgSpectrumStyle = 'ripple'` 重复，且页内只覆盖内容区，水面会在
 * 画布边缘断开，因此水面仅用于背景。
 *
 * 旧取值 `'off'`（与 `npSpectrum = false` 语义重复）由 `mergeSettings` 消化：
 * 落默认形态，不写回磁盘。
 */
export type SpectrumStyle = 'mirror' | 'radial' | 'ripple' | 'wave';

/** 设置页与 NowPlayingSpectrum 共用的形态清单（id 顺序即展示顺序）；label 存 i18n 键 */
export const SPECTRUM_STYLES: Array<{ value: SpectrumStyle; label: Msg }> = [
  { value: 'mirror', label: 'settings.spectrum.mirror' },
  { value: 'radial', label: 'settings.spectrum.radial' },
  { value: 'ripple', label: 'settings.spectrum.ripple' },
  { value: 'wave', label: 'settings.spectrum.wave' },
];

/**
 * 背景频谱光效档位。
 *
 * 审美开关，非性能开关：三档只改 shader 的衰减与线宽系数，开销差异可忽略。
 * "柔雾"（形状不可辨的光）与"锐光"（脊线清晰）互斥，故不固定单档。
 *
 * 山形与波纹共用该档位：两图层几何不同，档位语义仍统一为光的强度与边缘锐度；
 * 语义不统一会使同一档位在两种形态下表现不一致，被视为开关失效。
 * 键名 `ridgeGlow` 沿用山形时期的命名，改名会牵动迁移。
 */
export type RidgeGlow = 'soft' | 'balanced' | 'neon';

export const RIDGE_GLOWS: Array<{ value: RidgeGlow; label: Msg }> = [
  { value: 'soft', label: 'settings.ridgeGlow.soft' },
  { value: 'balanced', label: 'settings.ridgeGlow.balanced' },
  { value: 'neon', label: 'settings.ridgeGlow.neon' },
];

/**
 * 背景频谱形态。
 *
 * 与页内 `spectrumStyle` 分开：背景是常驻氛围（淡、范围大、可长时间观看），页内
 * 是一次性聚焦的视觉（可更亮更密），两者最优形态不同。共用一个键会迫使用户为了
 * 页内试形态而改动背景。
 *
 * - `ridge` 山形：闭合填充的环境光，边界柔和（默认）
 * - `ripple` 波纹：等深线层叠铺向下半屏，结构感强
 */
export type BgSpectrumStyle = 'ridge' | 'ripple';

export const BG_SPECTRUM_STYLES: Array<{ value: BgSpectrumStyle; label: Msg }> = [
  { value: 'ridge', label: 'settings.bgSpectrumStyle.ridge' },
  { value: 'ripple', label: 'settings.bgSpectrumStyle.ripple' },
];

export interface AppSettings {
  theme: ThemeMode;
  /** 主题颜色：固定渐变背景 + 配套强调色，成套切换（切换即换视觉身份） */
  ambient: AmbientScheme;
  /** 界面缩放档位（百分比，见 `UI_SCALES`）；`html` 内联 font-size 随它走（docs/56） */
  uiScale: UiScale;
  /**
   * 禁用系统缩放：开启后 Rust 侧把 webview zoom 补偿为 1/scale，内容
   * 按 100% 物理密度渲染（高分屏上字号更小、单屏内容更多、无位图缩放模糊），即时
   * 生效。与环境倍率（本开关）正交并相乘：环境倍率 × 界面缩放。zoom 由 Rust 的
   * `apply_scale_policy` 独占管理。
   */
  disableSystemScale: boolean;
  /** 按 ReplayGain 调整音量（近似实现：换算到音量系数） */
  normalizeVolume: boolean;
  /**
   * 全局 3D / WebGL 效果总闸。
   *
   * 能力闸门，非收藏夹：关闭时所有 WebGL 图层不渲染、不创建上下文，背景退回静态
   * CSS 渐变；`bgSpectrum` / `npSpectrum` 保留取值但一律无效。毛玻璃
   * （`backdrop-filter`）属 CSS 合成，不受该闸门控制。
   *
   * 存在理由：软件光栅（远程桌面、禁用硬件加速、老核显）下 WebGL 不可用，该环境
   * 无法在编译期识别；自动探测同样会误判，故保留用户可关闭的开关。
   */
  glEffects: boolean;
  /**
   * 背景动态频谱（山形环境光）。
   *
   * 开启时背景层订阅引擎 tap（`spectrum_set(true)`），关闭时只剩第 1 层静态渐变，
   * 引擎侧零开销。该开关同时决定毛玻璃成本：背景静止后 `backdrop-filter` 的结果被
   * 合成器缓存，玻璃降为一次性开销。
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
  /** 应用启动后自动播放上次的曲目 */
  autoplayOnStart: boolean;
  /** 记住上次播放进度；关闭则每次都从头开始 */
  rememberPosition: boolean;
  /** 播放时阻止系统休眠（允许熄屏） */
  preventSleep: boolean;
  /** 关闭窗口时最小化到托盘而不是退出 */
  closeToTray: boolean;
  /**
   * 自动刷新曲库的间隔（秒）。0 = 仅保留"切回窗口时刷新"。
   *
   * 可配而非固定 30 s：大库一次刷新需遍历数万条目录记录，需要更慢的节奏；频繁
   * 改标签的场景则需要更快。下限由宿主夹到 10 s——更短需改用文件监听（另一套机制）。
   */
  refreshSeconds: number;
  /**
   * 播放记录保留周期（天）。0 = 永久。
   *
   * 记录范围由该设置唯一定义：history 表只存周期内的行，宿主在启动、写历史、改
   * 设置时裁剪，行数由此收敛。「最近播放」tab 与常听读取裁剪后的 history；
   * 筛选器不设"上次播放"档。
   */
  historyDays: number;
  /**
   * 界面语言。空 = 首启未定（宿主按系统 UI 语言探测后写库，docs/64 §3.1），
   * 此后恒为 'zh' / 'en'——前端不做探测，只跟随。
   */
  language: string;
  /** 只存覆盖项：没写的动作走 `DEFAULT_SHORTCUTS`（改默认值不冲用户自定义） */
  shortcuts: Partial<Record<ShortcutAction, string>>;
  /**
   * 输出偏好（请求值）。设备实际给出的模式经 `audio:mode` 事件实时下发
   * （输出设置区展示），两者分开：请求
   * 独占而设备拒绝时，本字段仍为 `exclusive`。
   */
  output: DesiredOutput;
  /**
   * 直播页「最近打开」：上限 20，按 URL 去重，最新在前。
   *
   * 存于设置 KV 而非独立表：记录量级为 20 条，建表需额外的增删查命令与 IPC 类型；
   * KV 已具备落盘与迁移能力。
   */
  liveRecent: LiveRecentEntry[];
}

/** 直播页最近打开的一条 */
export interface LiveRecentEntry {
  /**
   * 稳定身份：与 URL 解耦——推流服务的容器重启可能换地址，流还是那条流。
   * UI 的 key / 移除 / 将来与服务端的同步都以它为准；URL 只承担"连哪里"。
   * 旧数据缺 id 时由 `normalizeLiveRecent` 按 URL 派生确定性 id 补齐。
   */
  id: string;
  url: string;
  /** 用户手改过的流名；缺省由宿主按 URL 推（`default_stream_title`） */
  title?: string;
  /** 副标题：悬浮信息层流名下的一行小字；缺省不显示 */
  subtitle?: string;
  /** 描述文本：悬浮信息层的说明（超三行截断，≤200 字） */
  description?: string;
  /**
   * 封面图（http(s) URL）：有则铺满卡面替代图标。暂无录入入口（不做内置
   * 上传，本地文件路径也未支持），为服务端站单与将来"选图存路径"预留
   */
  cover?: string;
  /**
   * 入库途径。manual = 手工添加；推流服务上线后其站单走 'service'，
   * 同步方向与覆盖优先级都以它判别。老数据缺省按 manual 归一。
   */
  /** manual = 手工添加；service = 服务端站单下发（推流频道，docs/68）。
   *  同步方向与覆盖优先级以它判别：service 卡的字段由服务端管，用户补填优先。 */
  source: 'manual' | 'service';
  /** 入库时间（毫秒）。重开/补元数据不改变它——它是"何时进库"不是"何时最后听" */
  addedAtMs: number;
}

export const DEFAULT_SETTINGS: AppSettings = {
  // 默认固定深色：跟随系统由用户显式选择，不作为初始外观
  theme: 'dark',
  ambient: 'aurora',
  // 100% = 设计原稿；高分屏可自行上调
  uiScale: 100,
  // 默认跟随系统缩放（Windows 惯例）；按 100% 物理密度渲染需显式开启
  disableSystemScale: false,
  normalizeVolume: false,
  // 总闸默认开：关闭会使全部 WebGL 效果失效，它不是性能选项。
  // 不支持的机器由 capability 探测兜底（探测失败仍可正常使用）
  glEffects: true,
  // 背景/页面频谱：背景默认开（山形环境光是应用的主视觉之一），页面默认关
  bgSpectrum: true,
  npSpectrum: false,
  // 默认环带
  spectrumStyle: 'radial',
  // 背景默认山形：常驻图层，不因新增形态而改变既有观感
  bgSpectrumStyle: 'ridge',
  // 默认柔雾：有氛围不刺眼
  ridgeGlow: 'soft',
  // 默认关：启动即出声需由用户显式开启
  autoplayOnStart: false,
  // 进度记忆默认开：播放器的通行行为
  rememberPosition: true,
  // 默认开：否则长时间播放时系统会休眠
  preventSleep: true,
  // 默认关：关窗即退出是既有行为
  closeToTray: false,
  // 30 秒：外部改标签/搬文件能在半分钟内体现，而一次增量扫描只有几十毫秒
  refreshSeconds: 30,
  // 90 天：兼顾轻量用户与"还在听"的语义
  historyDays: 90,
  // 空 = 首启未定；宿主首启按系统语言写库
  language: '',
  shortcuts: {},
  // 共享 + 200ms + 事件驱动：最稳妥的组合。独占需用户主动开启，
  // 它会锁住设备（其他程序无法出声）。
  output: { mode: 'shared', bufferMs: 200, clock: 'auto', releaseOnPause: true },
  // 空表起步：未打开过直播的机器不应出现记录
  liveRecent: [],
};
