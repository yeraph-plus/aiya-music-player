/**
 * 歌词高亮的时钟外推。
 *
 * 主进程的播放位置是 500ms 一次上报的。直接拿上报值算"当前唱到哪一句"，
 * 每句歌词最多会晚半秒才亮起来 —— 验收要求偏差 < 200ms，靠上报频率做不到。
 *
 * 所以渲染层保存一对锚点（上报位置 + 收到时的单调时钟），之后按真实流逝时间外推。
 * 每次新上报都会重新对时，因此误差不会累积，只受一帧（~16ms）限制。
 *
 * 抽成纯函数是为了能单测：外推本身没有副作用，可以脱离 DOM 验证。
 */

export interface ClockAnchor {
  /** 最近一次上报的播放位置（毫秒） */
  position: number;
  /** 收到该上报时的 performance.now() */
  at: number;
}

export function anchorAt(positionMs: number, now: number): ClockAnchor {
  return { position: Math.max(0, positionMs), at: now };
}

/**
 * 由锚点推算 now 时刻的播放位置。
 * durationMs > 0 时夹在时长内，避免暂停/结尾时越过整曲长度。
 */
export function projectPosition(anchor: ClockAnchor, now: number, durationMs = 0): number {
  const elapsed = Math.max(0, now - anchor.at);
  const next = anchor.position + elapsed;
  if (durationMs > 0) return Math.min(next, durationMs);
  return next;
}
