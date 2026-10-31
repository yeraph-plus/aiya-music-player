/**
 * 渲染能力探测（docs/46 §7）。
 *
 * 需要探测而非"有 WebGL 即使用"的原因：docs/46 §2.3 实测显示，软件光栅环境
 * （SwiftShader，对应远程桌面、禁用硬件加速、部分老核显）下 Canvas 2D 管线无法
 * 完成一次测量，而 WebGL shader 版 p95 为 1.3ms。因此"为兼容低端机保留 Canvas 2D"
 * 的直觉并不成立：两条路径都需保留，选择取决于具体机器。
 *
 * 探测结果分三档，只回答"该机器可达到哪一档"：
 *
 *   档 2  WebGL（硬件加速可用）      —— 流动渐变、山形光晕、页面频谱
 *   档 1  Canvas 2D（GL 不可用）      —— 两套 2D 实现，原样保留
 *   档 0  静态背景                    —— 仅 CSS 渐变，不执行逐帧绘制
 *
 * 探测本身只返回 2 或 1。软件光栅落档 1 而非档 0：软栅下 shader 版 p95 1.3ms，
 * 快于 Canvas 2D 管线，保留 2D 兜底优于全部关闭。档 0 仅由用户的「全局 3D 效果」
 * 开关触发（见 `AppShell` 的 `glEffects` 闸门）。
 *
 * 用户开关（`settings.glEffects`）单独计算：关闭时无论探测结果如何均走档 0。
 */

/** 渲染档位（不含用户开关） */
export type GlTier = 0 | 1 | 2;

export interface Capability {
  /** 该机器可达的最高档位 */
  tier: GlTier;
  /** 是否使用硬件加速。false 表示仅有软件光栅（SwiftShader、llvmpipe 等） */
  hardware: boolean;
  /** GPU 标识串，用于诊断与日志；无法获取时为空串 */
  renderer: string;
  /** 探测失败原因（仅用于日志） */
  note: string;
}

/**
 * `failIfMajorPerformanceCaveat: true` 是判断依据：Chromium 在仅可使用软件渲染时
 * 会直接返回 null，不需要匹配 renderer 字符串。字符串匹配留作二次确认——
 * 有些驱动会把软件实现报成硬件名，反之亦然。
 */
const SOFTWARE_HINT = /swiftshader|llvmpipe|basic render|software|microsoft basic/i;

let cached: Capability | null = null;

/**
 * 探测并缓存。只探测一次：每次调用都创建并销毁上下文，除耗时外，还会在上下文
 * 名额紧张时挤掉正在使用的上下文（Chromium 上限约 16 个）。
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

    // 先请求"必须硬件加速"的上下文：获取失败即说明仅有软件光栅
    const strict = canvas.getContext('webgl2', {
      failIfMajorPerformanceCaveat: true,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: 'high-performance',
    }) as WebGL2RenderingContext | null;

    if (!strict) {
      // 退一步请求不带性能门槛的 WebGL2：能获取即为软件光栅，落档 1
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
