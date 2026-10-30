/**
 * 逐帧绘制调度器：**全应用一个 rAF**（docs/46 §7）。
 *
 * 为什么不能各画各的：
 * 1. docs/26 踩过一个坑——`draw()` 里多写一次 `requestAnimationFrame(draw)`，
 *    调度数按 2^n 增长，几秒内主线程被回调淹没、整个 UI 假死（点击全部失效）。
 *    一个页面一个循环就已经够险，再叠"背景层 + 页面层 + 将来更多"必然出事。
 * 2. 三个 canvas 各自 rAF，同一帧里会被拆成三次独立的渲染事务，合成器要合并
 *    三次 dirty region；统一调度后，同一帧的绘制挨在一起，脏区一次提交。
 * 3. 后台/隐藏时只需要一处判断就能整体停摆，而不是每个组件各写一遍。
 *
 * 用法：`const off = subscribeRender((now) => draw(now)); … off();`
 * 订阅是幂等的——同一个函数重复订阅只算一次（Set 语义）。
 */

export type RenderTick = (now: number) => void;

const ticks = new Set<RenderTick>();
let frameId = 0;
let scheduled = false;
/** 页面隐藏时整体暂停（不是"少画"，是"完全不排帧"） */
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
 * 再排就是 docs/26 那个 2^n 事故的复现路径。
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
