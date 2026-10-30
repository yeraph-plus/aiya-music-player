/**
 * 设置迁移与合并（docs/46 §6.2）。
 *
 * 为什么从 `stores/ui.svelte.ts` 里抽出来：这段逻辑是**纯的**（输入磁盘 JSON、
 * 输出 AppSettings），但原先是 store 文件里的私有函数——想验证"旧配置能不能
 * 正确迁移"就只能靠手动改 blob 再重启，而这类 bug 的代价是用户配置丢失。
 * 抽成独立模块后可以直接在浏览器控制台/自动化里喂用例（见 docs/46 §6.2
 * 的三种旧组合），不必启动整条引导链。
 *
 * 三条约定：
 * 1. 旧键**解构剔除**，不跟着下一次 `setSettings` 写回磁盘（"停写"要真停）；
 * 2. 派生只在**新键缺失**时发生——已经在新版本里把背景关掉、页面留着的用户，
 *    下次启动不能被 blob 里的旧值翻回去；
 * 3. 形态/档位走白名单收口：blob 里出现不认识的字符串时落默认，不让它流进
 *    画笔的 switch（"类型说有四种、实际有第五种"是最难查的那类）。
 */

import { devValidate, settingsSchema } from './ipc/schema';
import {
  DEFAULT_SETTINGS,
  BG_SPECTRUM_STYLES,
  RIDGE_GLOWS,
  SPECTRUM_STYLES,
  type AppSettings,
} from './settings';

const SPECTRUM_STYLE_VALUES: ReadonlySet<string> = new Set(SPECTRUM_STYLES.map((s) => s.value));
const BG_SPECTRUM_STYLE_VALUES: ReadonlySet<string> = new Set(BG_SPECTRUM_STYLES.map((s) => s.value));
const RIDGE_GLOW_VALUES: ReadonlySet<string> = new Set(RIDGE_GLOWS.map((s) => s.value));

/**
 * 已退役的键：**读时消化，写时剔除**。
 *
 * 为什么必须显式删：`{ ...DEFAULT_SETTINGS, ...raw }` 会把 raw 里的未知键原样带进
 * 结果对象，于是用户改任何一个设置，这些死键就跟着 `setSettings` 再写回磁盘一次
 * ——"停写"实际没停，只是从没被读过。实测本机库里的 `app_settings` 至今还带着
 * `spectrumEngine` 与 `accentFromArtwork`（docs/26 说要清、当时漏了），就是这条。
 */
const RETIRED_KEYS = ['spectrum', 'spectrumEngine', 'accentFromArtwork'] as const;

/**
 * 动态 JSON → `AppSettings`：以默认值为底合并 + 旧键迁移。
 *
 * 为什么不直接断言成 `AppSettings`：老配置文件里没有 `output`（2026-10 才加），
 * 直接断言会让 `settings.output.mode` 在运行期是 undefined —— 类型说有、
 * 实际没有，是这类 bug 最难查的形态。
 */
export function mergeSettings(raw: unknown): AppSettings {
  devValidate(settingsSchema, raw, 'app.settings');
  const obj = (typeof raw === 'object' && raw !== null ? raw : {}) as Record<string, unknown>;

  // 取值要在剔除**之前**（剔除后就只剩 undefined 了）
  const legacySpectrum = obj.spectrum;
  const legacyStyle = obj.spectrumStyle;
  const rest: Record<string, unknown> = { ...obj };
  for (const key of RETIRED_KEYS) delete rest[key];

  const merged: AppSettings = { ...DEFAULT_SETTINGS, ...(rest as Partial<AppSettings>) };

  // 形态更名（2026-10-30）：`'wave'`（心电图）退役，席位由 `'ripple'`（中轴波纹）
  // 接管。这里**显式映射**而不是靠白名单落回默认——用户当初选的就是"折线感"，
  // 落回镜像曲线等于把手里的选择改掉。必须在白名单检查之前跑，否则 wave 先被
  // 判为不认识、直接变成 'mirror'。
  // （比较走 String()：`merged` 是从磁盘 JSON 强转来的，类型说 SpectrumStyle
  //  并不代表运行期不会出现 'wave'——这正是这次要处理的情况。）
  if (String(merged.spectrumStyle) === 'wave') merged.spectrumStyle = 'ripple';

  // 形态/档位白名单：旧配置里的 `'off'` 也走这条（它已不在 SPECTRUM_STYLES 里），
  // 于是自动落回默认形态——退役取值要靠**不在白名单**这件事被消化掉，而不是
  // 到处写 `if (x === 'off')`（那种写法会在加第四种形态时漏掉）。
  if (!SPECTRUM_STYLE_VALUES.has(String(merged.spectrumStyle))) {
    merged.spectrumStyle = DEFAULT_SETTINGS.spectrumStyle;
  }
  if (!RIDGE_GLOW_VALUES.has(String(merged.ridgeGlow))) {
    merged.ridgeGlow = DEFAULT_SETTINGS.ridgeGlow;
  }
  // 背景形态是新键：老配置里根本没有它（`...DEFAULT_SETTINGS` 已经兜成山形），
  // 这里只负责挡住将来写坏的值。
  if (!BG_SPECTRUM_STYLE_VALUES.has(String(merged.bgSpectrumStyle))) {
    merged.bgSpectrumStyle = DEFAULT_SETTINGS.bgSpectrumStyle;
  }

  if (typeof legacySpectrum === 'boolean') {
    if (typeof rest.bgSpectrum !== 'boolean') merged.bgSpectrum = legacySpectrum;
    if (typeof rest.npSpectrum !== 'boolean') {
      // 用**原始** spectrumStyle 判断：'off' 的旧配置 = 当时页面上没画，
      // 此时不能因为上面已经把它改写成 'mirror' 就误判成"页面开着"
      merged.npSpectrum = legacySpectrum && String(legacyStyle) !== 'off';
    }
  }

  return merged;
}
