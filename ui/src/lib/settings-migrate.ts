/**
 * 设置迁移与合并。
 *
 * 纯函数：输入磁盘 JSON，输出 `AppSettings`。独立成模块以便直接喂用例验证
 * 无需启动引导链；迁移错误的代价是用户配置丢失。
 *
 * 三条约定：
 * 1. 旧键在读时剔除，不随下一次 `setSettings` 写回磁盘；
 * 2. 派生值只在新键缺失时产生——已在新版本中关闭背景、保留页内频谱的配置，重启后
 *    不被 blob 中的旧值覆盖；
 * 3. 形态与档位经白名单收口：不认识的字符串落默认值，不进入画笔的 switch
 *    （"类型声明四种、运行期出现第五种"是难定位的一类问题）。
 */

import { devValidate, settingsSchema } from './ipc/schema';
import {
  DEFAULT_SETTINGS,
  BG_SPECTRUM_STYLES,
  RIDGE_GLOWS,
  SPECTRUM_STYLES,
  normalizeUiScale,
  type AppSettings,
} from './settings';

const SPECTRUM_STYLE_VALUES: ReadonlySet<string> = new Set(SPECTRUM_STYLES.map((s) => s.value));
const BG_SPECTRUM_STYLE_VALUES: ReadonlySet<string> = new Set(BG_SPECTRUM_STYLES.map((s) => s.value));
const RIDGE_GLOW_VALUES: ReadonlySet<string> = new Set(RIDGE_GLOWS.map((s) => s.value));

/**
 * 已退役的键：读时消化，写时剔除。
 *
 * 剔除是必需的：`{ ...DEFAULT_SETTINGS, ...raw }` 会把 raw 的未知键带入结果对象，
 * 用户修改任一设置时这些死键随 `setSettings` 再写回磁盘一次，"停写"实际未停。
 *
 * `spectrum` / `spectrumEngine` / `accentFromArtwork` 来自频谱与封面取色的旧方案；
 * `density` 因行高只保留一套；`glass` 无消费方（壳层不做玻璃）。写回路径是整个
 * 对象快照，残留一个死键即永久写回。
 */
const RETIRED_KEYS = ['spectrum', 'spectrumEngine', 'accentFromArtwork', 'density', 'glass'] as const;

/**
 * 动态 JSON → `AppSettings`：以默认值为底合并，并迁移旧键。
 *
 * 不做直接断言：旧配置可能缺少 `output` 等后加字段，断言会使
 * `settings.output.mode` 在运行期为 undefined（类型声明存在、运行期不存在）。
 */
export function mergeSettings(raw: unknown): AppSettings {
  devValidate(settingsSchema, raw, 'app.settings');
  const obj = (typeof raw === 'object' && raw !== null ? raw : {}) as Record<string, unknown>;

  // 取值须在剔除之前（剔除后只剩 undefined）
  const legacySpectrum = obj.spectrum;
  const legacyStyle = obj.spectrumStyle;
  const rest: Record<string, unknown> = { ...obj };
  for (const key of RETIRED_KEYS) delete rest[key];

  const merged: AppSettings = { ...DEFAULT_SETTINGS, ...(rest as Partial<AppSettings>) };

  // 形态更名：`'wave'`（心电图）退役，由 `'ripple'`（中轴波纹）接管。这里显式映射**而不是靠白名单落回默认——用户原选择是"折线感"，落回镜像曲线等于改掉其选择。必须早于白名单检查，否则 'wave' 先被判为不认识而变成
  // 'mirror'。（比较走 String()：`merged` 由磁盘 JSON 强转而来，类型声明为 SpectrumStyle 不代表运行期不会出现 'wave'。）
  if (String(merged.spectrumStyle) === 'wave') merged.spectrumStyle = 'ripple';

  // 形态/档位白名单：旧配置里的 `'off'` 也走这条（它已不在 SPECTRUM_STYLES
  // 里），于是自动落回默认形态——退役取值要靠**不在白名单**这件事被消化掉，而不是到处写 `if (x === 'off')`（那种写法会在加第四种形态时漏掉）。
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

  // 界面缩放档位：数值白名单收口。手改配置里出现 87.5 这类值时
  // 落回 100，不让未知档位流进 look/DOM（档位语义见 settings.ts 的 UI_SCALES）。
  merged.uiScale = normalizeUiScale(merged.uiScale);

  // 「最近打开」是新键：老配置里没有它（`...DEFAULT_SETTINGS` 兜成空表），这里只负责挡住被手改成别的形状的值——`AppSettings`
  // 说是数组，运行期从磁盘 JSON 强转来的可能是个对象，下游 `.length` 会静默变成 undefined。
  if (!Array.isArray(merged.liveRecent)) merged.liveRecent = DEFAULT_SETTINGS.liveRecent;

  if (typeof legacySpectrum === 'boolean') {
    if (typeof rest.bgSpectrum !== 'boolean') merged.bgSpectrum = legacySpectrum;
    if (typeof rest.npSpectrum !== 'boolean') {
      // 判据用**原始** spectrumStyle：'off' 表示旧配置下页面未渲染频谱。若改用
      // 已归一化为 'mirror' 的值，会被误判为"页面开着"。
      merged.npSpectrum = legacySpectrum && String(legacyStyle) !== 'off';
    }
  }

  return merged;
}
