/**
 * 逐帧绘制调度器：全应用一个 rAF。
 *
 * 统一调度而非各自调度的原因：
 * 1. 在 `draw()` 内再次调用 `requestAnimationFrame(draw)` 会使调度数按 2^n 增长，
 *    数秒内回调占满主线程，界面完全无响应（点击失效）。单页面单循环已接近上限，
 *    叠加背景层与页面层后必然发生。
 * 2. 多个 canvas 各自 rAF 时，同一帧被拆成多次独立渲染事务，合成器需合并多个
 *    dirty region；统一调度后同帧绘制连续提交，脏区一次合并。
 * 3. 后台与隐藏状态只需一处判断即可整体停摆，无需每个组件各写一遍。
 *
 * 用法：`const off = subscribeRender((now) => draw(now)); … off();`
 * 订阅幂等：同一函数重复订阅只计一次（Set 语义）。
 */

export type RenderTick = (now: number) => void;

const ticks = new Set<RenderTick>();
let frameId = 0;
let scheduled = false;
/** 页面隐藏时整体暂停：不排帧，而非降低帧率 */
let paused = typeof document !== 'undefined' && document.hidden;

function schedule(): void {
  if (scheduled || paused || ticks.size === 0) return;
  scheduled = true;
  frameId = requestAnimationFrame(pump);
}

function pump(now: number): void {
  frameId = 0;
  scheduled = false;

  // 快照再遍历：tick 内部可能订阅/退订（例如某一层自己决定收工），
  // 直接迭代 Set 会在修改时跳过后面的订阅者
  for (const tick of [...ticks]) {
    if (!ticks.has(tick)) continue;
    try {
      tick(now);
    } catch (error) {
      // 一个图层画挂了不该拖垮其它图层（更不能让整个应用停止绘制）
      console.error('render tick failed:', error);
    }
  }

  schedule();
}

/**
 * 订阅逐帧回调，返回退订函数。
 *
 * 注意 tick 里**不要**再调 `requestAnimationFrame`——调度器已经保证每帧一次，
 * 再排就是 那个 2^n 事故的复现路径。
 */
export function subscribeRender(tick: RenderTick): () => void {
  ticks.add(tick);
  schedule();
  return () => {
    ticks.delete(tick);
    if (ticks.size === 0) {
      cancelAnimationFrame(frameId);
      frameId = 0;
      scheduled = false;
    }
  };
}

/** 当前订阅者数量（调试/测试用） */
export function renderTickCount(): number {
  return ticks.size;
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    paused = document.hidden;
    if (paused) {
      cancelAnimationFrame(frameId);
      frameId = 0;
      scheduled = false;
    } else {
      schedule();
    }
  });
}
