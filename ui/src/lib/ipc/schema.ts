/**
 * 动态边界的运行期校验（docs/17 §6）：**只有这三类**才用 zod。
 *
 * 为什么不全量校验：命令的返回类型已经由 Rust 生成（`generated/`），
 * 再写一遍 schema 是重复劳动，改一处要改两处。真正需要运行期校验的是
 * 「形状会演化、且旧数据要兼容」的载荷——它们**没有** Rust DTO。
 *
 * 为什么热路径禁止校验：`player:state`（10Hz）、`spectrum:frame`（30fps）
 * 每帧过一次 zod 会直接吃掉渲染预算；那两条靠生成的静态类型 + Rust 快照测试。
 */
// 必须先于本文件的 `z.object(...)`：zod v4 在构造 schema 时判定 JIT，而判定会
// 缓存，写晚了改不回来（原因与实测见 zod-setup.ts）
import '../zod-setup';
import { z } from 'zod';

/** 设置项。老配置文件缺字段是常态（`output` 是 2026-10 才加的），所以全 optional + passthrough。 */
export const settingsSchema = z
  .object({
    theme: z.enum(['dark', 'light']).optional(),
    /** 主题颜色（2026-10-26 起取代 accentFromArtwork；旧字段不再写） */
    ambient: z.enum(['aurora', 'ember', 'meadow', 'dusk', 'mono']).optional(),
    normalizeVolume: z.boolean().optional(),
    /** 2026-10-30 前的单开关：**只读不写**，`mergeSettings` 用它派生两个新键后丢弃 */
    spectrum: z.boolean().optional(),
    /** 全局 3D 效果总闸（docs/46 §6.2）；缺省时按默认开兜底 */
    glEffects: z.boolean().optional(),
    /** 背景动态频谱 / 正在播放页频谱：2026-10-30 拆开（docs/46 §5） */
    bgSpectrum: z.boolean().optional(),
    npSpectrum: z.boolean().optional(),
    /** 山形光晕档位（docs/46 §3.2） */
    ridgeGlow: z.enum(['soft', 'balanced', 'neon']).optional(),
    /**
     * 频谱形态。`off` / `wave` 在这里保留：它们是**旧配置里的存量值**，读进来
     * 要能过校验（再由 mergeSettings 迁移）。新代码不再写这两个值。
     */
    spectrumStyle: z.enum(['mirror', 'radial', 'wave', 'ripple', 'off']).optional(),
    /** 背景频谱形态（2026-10-30 新增；缺省按山形兜底） */
    bgSpectrumStyle: z.enum(['ridge', 'ripple']).optional(),
    mediaKeys: z.boolean().optional(),
    autoplayOnStart: z.boolean().optional(),
    rememberPosition: z.boolean().optional(),
    preventSleep: z.boolean().optional(),
    closeToTray: z.boolean().optional(),
    /** 自动刷新间隔（秒，0 = 关掉定时；下限由宿主夹到 10） */
    refreshSeconds: z.number().int().min(0).max(3600).optional(),
    shortcuts: z.record(z.string(), z.string()).optional(),
    /** 输出配置（2026-10 起取代 `exclusiveMode: bool`） */
    output: z
      .object({
        mode: z.enum(['shared', 'exclusive', 'exclusiveRequired', 'null']).optional(),
        bufferMs: z.number().int().min(0).max(2000).optional(),
        clock: z.enum(['auto', 'events', 'polling']).optional(),
        releaseOnPause: z.boolean().optional(),
      })
      .passthrough()
      .optional(),
    /** v1 遗留字段：只在新代码回落时读它，不再写 */
    exclusiveMode: z.boolean().optional(),
  })
  .passthrough();

export type Settings = z.infer<typeof settingsSchema>;

/** UI 状态（视图/滚动/面板）。写坏了大不了回到默认视图，所以校验失败可静默。 */
export const uiStateSchema = z
  .object({
    sort: z.enum(['title', 'added', 'artist', 'album']).optional(),
    direction: z.enum(['asc', 'desc']).optional(),
    scrollTop: z.number().optional(),
    artistId: z.string().nullable().optional(),
    albumId: z.string().nullable().optional(),
    /**
     * 一级路由（B4-1 拆页 8 值；B4-3 加专辑/艺人/搜索；docs/28 撤
     * recent/mostPlayed；docs/29 批 A 加 tools 共 12 值）。旧文件的 `home`
     * 仍合法；落到已退役的 `recent`/`mostPlayed` 时由 ui.restore() 换算成
     * 曲库（这里不收编，让 devValidate 在开发期把"还在写退役值"的代码暴露出来）。
     */
    page: z
      .enum([
        'home',
        'library',
        'playlist',
        'albums',
        'artists',
        'album',
        'artist',
        'search',
        'nowPlaying',
        'settings',
        'tools',
        'radio',
      ])
      .optional(),
    /** `page === 'playlist'` 的参数 */
    playlistId: z.string().nullable().optional(),
    /** 拆页（2026-10-26）之前的两个字段：只在新字段缺失时用于换算，不再写回 */
    view: z.string().optional(),
    libraryDetail: z.boolean().optional(),
  })
  .passthrough();

export type UiState = z.infer<typeof uiStateSchema>;

/**
 * 开发期校验：解析失败不抛（否则开发时一个字段笔误就白屏），
 * 打一条带路径的错误 + 返回原值，让调用方继续跑。
 *
 * 生产构建里 `import.meta.env.DEV` 是 false，整个分支会被摇掉。
 */
export function devValidate<T>(schema: z.ZodType<T>, raw: unknown, label: string): unknown {
  if (!import.meta.env.DEV) return raw;
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    console.error(`[ipc] ${label} 形状不符（依此修 DTO 或 schema）：`, parsed.error.issues);
  }
  return raw;
}
