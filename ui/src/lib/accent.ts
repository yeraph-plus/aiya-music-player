/**
 * 主题色变量的读取缓存。
 *
 * **为什么需要**：频谱/环境光/可视化面板的 canvas 绘制循环每帧都要取
 * `--accent`（60fps 下每秒 60 次 `getComputedStyle`）。`getComputedStyle`
 * 会强制一次样式解析并返回一个活对象，在动画期间是实打实的每帧开销；
 * 而那些值只在切主题 / 换背景方案时变一次 —— 用「每帧重取」去跟随一个
 * 秒级才变的值，代价与收益不成比例。
 *
 * **失效信号**：`<html>` 上的 `data-theme` / `data-ambient` 属性。
 * 用 MutationObserver 而不是定时重读：换主题后必须**立刻**失效，否则光谱
 * 会保持旧色到下一次偶然重取（曾经就是"切了主题但频谱不变色"）。
 *
 * 前提：`--accent` / `--text-2` / `--text-3` 全部定义在 `html` 选择器上
 * （tokens.css 的 :root、html[data-theme=...]、html[data-ambient=...]）。
 * 若将来在某个容器上做局部覆盖，这里读 documentElement 会取不到 —— 那时
 * 需要把 element 参数加回来，别默默改成读 canvas 祖先。
 */

/** 与 tokens.css 的 :root 默认值一致：缓存失效瞬间也不会闪成透明 */
const FALLBACKS: Readonly<Record<string, string>> = {
  '--accent': '#7c9cff',
  '--text-2': 'rgba(128,128,128,.8)',
  '--text-3': 'rgba(128,128,128,.25)',
};

const cache = new Map<string, string>();
const listeners = new Set<() => void>();
let observer: MutationObserver | null = null;

function invalidate(): void {
  cache.clear();
  for (const fn of listeners) fn();
}

function ensureObserver(): void {
  if (observer || typeof MutationObserver === 'undefined') return;
  observer = new MutationObserver(invalidate);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'data-ambient', 'class', 'style'],
  });
}

/**
 * 取一个主题变量的当前计算值（带缓存）。
 *
 * 传 `fallback` 只在变量缺失时用；内置的 `--accent` / `--text-2` / `--text-3`
 * 不传也会回退到与 tokens.css 一致的默认值。
 */
export function themeColor(name: string, fallback?: string): string {
  const hit = cache.get(name);
  if (hit !== undefined) return hit;
  ensureObserver();
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  // 取到空值（样式表还没解析完，dev 下 HMR 重载时会出现）**不进缓存**：
  // 否则会一直用兜底色画到下次换主题，而"换主题"可能几个小时都不发生。
  if (!raw) return fallback || FALLBACKS[name] || '';
  cache.set(name, raw);
  return raw;
}

/**
 * 订阅主题变化（切主题 / 换背景方案）。返回取消订阅函数。
 *
 * canvas 绘制器用它重建依赖颜色的资源（渐变贴图、离屏缓存），
 * 而不是在每帧里重新读变量。
 */
export function onThemeChange(fn: () => void): () => void {
  ensureObserver();
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}
