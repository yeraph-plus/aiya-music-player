/**
 * 首帧外观（主题 + 主题颜色）。
 *
 * 为什么要有这一层：主题的**真值在 SQLite**（`app_settings` 表），要等
 * `ui.bootstrap()` 走一次 IPC 才拿得到 —— 那时组件已经挂载完了。而
 * `<html data-theme>` 必须在**第一帧之前**就设对，否则浅色主题会先画一帧
 * 深色再跳成浅色（深→浅的闪白尤其明显）。
 *
 * 做法是写一份 localStorage 镜像：`App` 的主题 `$effect` 每次应用外观时顺手
 * 写回，`main.ts` 挂载前先读。镜像与库里不一致时最多闪一帧，紧接着
 * `bootstrap()` 返回真值就会纠正——比"每次都闪"好。
 *
 * `window.__AIYA_LOOK__` 是给"主进程往 HTML 里预置首帧数据"留的入口。
 * 实测（`grep __AIYA_LOOK__ src-tauri` 为空）主进程**没有**注入它，
 * 所以 Tauri 下这条分支目前不生效；保留是为了将来若真的在 `index.html`
 * 里预置时这里零改动。预览模式下 mock 会写它。
 */
import { normalizeUiScale, type AmbientScheme, type ThemeMode, type UiScale } from './settings';

export interface Look {
  theme: ThemeMode;
  /** 主题颜色：切 `html[data-ambient]`，CSS 渐变与强调色成套换 */
  ambient: AmbientScheme;
  /** 界面缩放档位（docs/56）：随主题一起走首帧镜像；readLook 已收口，恒为合法档位 */
  uiScale: UiScale;
}

const KEY = 'aiya.look';

/** 兜底值必须与 `DEFAULT_SETTINGS` 一致：镜像缺失时首帧就是默认外观 */
const FALLBACK: Look = { theme: 'dark', ambient: 'aurora', uiScale: 100 };

const AMBIENTS: ReadonlySet<string> = new Set(['aurora', 'ember', 'meadow', 'dusk', 'mono']);

function isTheme(value: unknown): value is ThemeMode {
  return value === 'dark' || value === 'light';
}

function isAmbient(value: unknown): value is AmbientScheme {
  return typeof value === 'string' && AMBIENTS.has(value);
}

/**
 * 读首帧外观。三级回退：localStorage 镜像 → `__AIYA_LOOK__` → 默认值。
 *
 * 全程 try/catch：隐私模式或存储被禁用时 `localStorage.getItem` 会抛
 * `SecurityError`，而这段代码在挂载前跑，抛出去就是白屏。
 */
export function readLook(): Look {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<Look>;
      if (isTheme(parsed.theme)) {
        // ambient / uiScale 是后加字段：镜像里没有就落默认（不为此判镜像无效）
        return {
          theme: parsed.theme,
          ambient: isAmbient(parsed.ambient) ? parsed.ambient : FALLBACK.ambient,
          uiScale: normalizeUiScale(parsed.uiScale),
        };
      }
    }
  } catch {
    /* 存储不可用或 JSON 损坏：落到下一级 */
  }

  const injected = window.__AIYA_LOOK__;
  if (injected && isTheme(injected.theme)) {
    return {
      theme: injected.theme,
      ambient: isAmbient(injected.ambient) ? injected.ambient : FALLBACK.ambient,
      uiScale: normalizeUiScale(injected.uiScale),
    };
  }
  return FALLBACK;
}

/**
 * 写回镜像。写失败不影响本次外观（`applyLook` 已经把值写进 DOM 了），
 * 只是下次启动会闪一帧，所以静默忽略。
 */
export function writeLook(look: Look): void {
  try {
    localStorage.setItem(
      KEY,
      JSON.stringify({ theme: look.theme, ambient: look.ambient, uiScale: look.uiScale }),
    );
  } catch {
    /* 配额满/存储禁用：只影响下次首帧 */
  }
}

/**
 * 读当前列表行高（px）。
 *
 * **CSS 是唯一出处**：`--row-h` 定义在 tokens.css，虚拟列表要一个数字
 * （算可见区间、拖拽落点），于是这里读一次。不要在组件里写死 52 ——
 * 改了 CSS 忘改 JS 的症状是列表越滚越偏，不报错。
 *
 * 不能 parseFloat `getComputedStyle().getPropertyValue()`：自定义属性的
 * 计算值**不换算单位**，`--row-h` 改 rem（docs/56）后只会原样拿到
 * "3.25rem"，parseFloat 得 3.25，列表静默错位。改为探针测量：让引擎把
 * `var(--row-h)` 解析成真实 px，界面缩放后自动返回缩放值（可能是小数，
 * 不要 round——行高与虚拟列表的步长必须同源，round 会逐行积累 0.x px 漂移）。
 *
 * 密度设置已移除（2026-10-30）：只有一套行高，`--row-h` 不再按属性分档。
 */
export function readRowHeight(): number {
  const probe = document.createElement('div');
  probe.style.cssText = 'position:absolute;visibility:hidden;pointer-events:none;height:var(--row-h)';
  document.body.appendChild(probe);
  const px = probe.getBoundingClientRect().height;
  probe.remove();
  return Number.isFinite(px) && px > 0 ? px : 52;
}

/**
 * 把外观写到 `<html>`（CSS 变量按 `[data-theme]` / `[data-ambient]` 切换；
 * 界面缩放走 `html` 内联 font-size，tokens.css 的 rem 尺寸轴整体跟随）。
 * 100 档也照写 16px：与"没设过"完全同值，省一条分支。
 */
export function applyLook(look: Look): void {
  document.documentElement.dataset.theme = look.theme;
  document.documentElement.dataset.ambient = look.ambient;
  document.documentElement.style.fontSize = `${(16 * look.uiScale) / 100}px`;
}
