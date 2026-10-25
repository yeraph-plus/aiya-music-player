/**
 * 首帧外观（主题 + 密度）。
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
import type { ListDensity, ThemeMode } from './settings';

export interface Look {
  theme: ThemeMode;
  density: ListDensity;
}

const KEY = 'aiya.look';

/** 兜底值必须与 `DEFAULT_SETTINGS` 一致：镜像缺失时首帧就是默认外观 */
const FALLBACK: Look = { theme: 'dark', density: 'comfortable' };

function isTheme(value: unknown): value is ThemeMode {
  return value === 'dark' || value === 'light';
}

function isDensity(value: unknown): value is ListDensity {
  return value === 'comfortable' || value === 'compact';
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
      if (isTheme(parsed.theme) && isDensity(parsed.density)) {
        return { theme: parsed.theme, density: parsed.density };
      }
    }
  } catch {
    /* 存储不可用或 JSON 损坏：落到下一级 */
  }

  const injected = window.__AIYA_LOOK__;
  if (injected && isTheme(injected.theme) && isDensity(injected.density)) {
    return { theme: injected.theme, density: injected.density };
  }
  return FALLBACK;
}

/**
 * 写回镜像。写失败不影响本次外观（`applyLook` 已经把值写进 DOM 了），
 * 只是下次启动会闪一帧，所以静默忽略。
 */
export function writeLook(look: Look): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ theme: look.theme, density: look.density }));
  } catch {
    /* 配额满/存储禁用：只影响下次首帧 */
  }
}

/** 把外观写到 `<html>`（CSS 变量按 `[data-theme]` / `[data-density]` 切换） */
export function applyLook(look: Look): void {
  document.documentElement.dataset.theme = look.theme;
  document.documentElement.dataset.density = look.density;
}
