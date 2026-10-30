/**
 * 渲染能力探测（docs/46 §7）。
 *
 * 为什么需要一层探测，而不是"有 WebGL 就用"：
 *
 * docs/46 §2.3 实测——在软件光栅（SwiftShader，等价于远程桌面 / 禁用硬件加速 /
 * 部分老核显）下，**当前这套 Canvas 2D 管线会慢到跑不完一次测量**，而 WebGL
 * shader 版 p95 只有 1.3ms。也就是说"为了兼容低端机而留在 Canvas 2D"这个直觉
 * 是反的：两边都得有，选哪边取决于机器。
 *
 * 于是分三档，探测只回答"这台机器能到哪一档"：
 *
 *   档 2  WebGL（硬件加速可用）      —— 流动渐变 + 山形光晕 + 页面频谱
 *   档 1  Canvas 2D（GL 不可用）      —— 现有的两套 2D 实现，原样保留
 *   档 0  静态背景（只关不掉动画时）  —— 只剩 CSS 渐变，不跑任何逐帧绘制
 *
 * 注意：**探测本身只会返回 2 或 1**。软件光栅（SwiftShader / 禁用硬件加速）
 * 落到档 1 而不是档 0 —— 那是 docs/46 §2.3 实测的结论：软栅下 shader 版 p95
 * 1.3ms，反而比 Canvas 2D 管线快，所以留着 2D 兜底比全关更合理。档 0 只由
 * 用户的「全局 3D 效果」开关触发（见 `AppShell` 的 `glEffects` 闸门）。
 *
 * 用户开关（`settings.glEffects`）在这之上另算：关掉时无论探测结果如何都走档 0。
 */

/** 渲染档位（不含用户开关） */
export type GlTier = 0 | 1 | 2;

export interface Capability {
  /** 这台机器能到的最高档位 */
  tier: GlTier;
  /** 是否走硬件加速。false 表示只有软件光栅（SwiftShader / llvmpipe 之类） */
  hardware: boolean;
  /** GPU 标识串，诊断/日志用；拿不到时为空串 */
  renderer: string;
  /** 探测失败的原因（仅日志用） */
  note: string;
}

/**
 * `failIfMajorPerformanceCaveat: true` 是关键：Chromium 在"只有软件渲染可用"时
 * 会直接返回 null，不需要匹配 renderer 字符串。字符串匹配留作二次确认——
 * 有些驱动会把软件实现报成硬件名，反之亦然。
 */
const SOFTWARE_HINT = /swiftshader|llvmpipe|basic render|software|microsoft basic/i;

let cached: Capability | null = null;

/**
 * 探测并缓存。**只探一次**：每次调都创建/销毁上下文的话，代价不说，还会在
 * 上下文名额紧张时把真正在用的那个挤掉（Chromium 上限 ~16 个）。
 */
export function detectCapability(): Capability {
  if (cached) return cached;
  cached = probe();
  return cached;
}

/** 仅测试用：清缓存，让下一次 `detectCapability()` 重新探测 */
export function resetCapabilityCache(): void {
  cached = null;
}

function probe(): Capability {
  if (typeof document === 'undefined') {
    return { tier: 1, hardware: false, renderer: '', note: 'no document' };
  }

  let canvas: HTMLCanvasElement | null = null;
  let gl: WebGL2RenderingContext | WebGLRenderingContext | null = null;

  try {
    canvas = document.createElement('canvas');
    canvas.width = 1;
    canvas.height = 1;

    // 先按"必须硬件加速"要一个：拿不到就说明只有软件光栅
    const strict = canvas.getContext('webgl2', {
      failIfMajorPerformanceCaveat: true,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: 'high-performance',
    }) as WebGL2RenderingContext | null;

    if (!strict) {
      // 退一步问问"有没有 WebGL，不管快慢"——有的话是软件光栅，走档 1
      const any = canvas.getContext('webgl2') as WebGL2RenderingContext | null;
      const renderer = any ? readRenderer(any) : '';
      if (any) release(any);
      return {
        tier: 1,
        hardware: false,
        renderer,
        note: 'no hardware-accelerated webgl2',
      };
    }

    gl = strict;
    const renderer = readRenderer(strict);
    release(strict);

    if (SOFTWARE_HINT.test(renderer)) {
      return { tier: 1, hardware: false, renderer, note: 'software renderer string' };
    }

    return { tier: 2, hardware: true, renderer, note: '' };
  } catch (error) {
    return {
      tier: 1,
      hardware: false,
      renderer: '',
      note: error instanceof Error ? error.message : 'probe threw',
    };
  } finally {
    // 释放探测用的上下文：它只用来问一句话，不该占着名额
    if (gl) release(gl);
    canvas = null;
  }
}

function readRenderer(gl: WebGLRenderingContext | WebGL2RenderingContext): string {
  try {
    const ext = gl.getExtension('WEBGL_debug_renderer_info');
    if (ext) {
      return String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) ?? '');
    }
    return String(gl.getParameter(gl.RENDERER) ?? '');
  } catch {
    return '';
  }
}

/**
 * 主动丢弃上下文。不丢的话，探测一次就在进程里留一个"活的" WebGL 上下文，
 * 多探几次会撞 Chromium 的上限，最坏情况是把正在渲染的那个挤掉——症状是
 * 别人的画面突然变黑，而且极难归因。
 */
function release(gl: WebGLRenderingContext | WebGL2RenderingContext): void {
  try {
    gl.getExtension('WEBGL_lose_context')?.loseContext();
  } catch {
    /* 已经丢了/扩展不支持：无所谓 */
  }
}
