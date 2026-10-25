export function formatTime(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms <= 0) return '0:00';
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

export function formatDurationLong(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '0 分钟';
  const minutes = Math.round(ms / 60000);
  if (minutes < 60) return `${minutes} 分钟`;
  const hours = Math.floor(minutes / 60);
  return `${hours} 小时 ${minutes % 60} 分钟`;
}

export interface HighlightPart {
  text: string;
  match: boolean;
}
/**
 * 搜索命中高亮。
 * 用原始标题与查询做小写比较，而不是比对 title_norm：
 * 归一化后的字符串下标与展示字符串对不上，高亮会错位。
 */
export function highlight(text: string, query: string): HighlightPart[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [{ text, match: false }];

  const haystack = text.toLowerCase();
  const parts: HighlightPart[] = [];
  let cursor = 0;

  for (;;) {
    const at = haystack.indexOf(needle, cursor);
    if (at < 0) break;
    if (at > cursor) parts.push({ text: text.slice(cursor, at), match: false });
    parts.push({ text: text.slice(at, at + needle.length), match: true });
    cursor = at + needle.length;
  }

  if (cursor < text.length) parts.push({ text: text.slice(cursor), match: false });
  return parts.length > 0 ? parts : [{ text, match: false }];
}

export function repeatIcon(mode: 'off' | 'all' | 'one'): string {
  if (mode === 'one') return '🔂';
  if (mode === 'all') return '🔁';
  return '🔁';
}

export function repeatLabel(mode: 'off' | 'all' | 'one'): string {
  if (mode === 'one') return '单曲循环';
  if (mode === 'all') return '列表循环';
  return '顺序播放';
}

export function statusLabel(status: string): string {
  switch (status) {
    case 'loading':
      return '正在载入…';
    case 'playing':
      return '正在播放';
    case 'paused':
      return '已暂停';
    case 'error':
      return '播放出错';
    default:
      return '';
  }
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatDateTime(timestamp: number): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '—';
  const date = new Date(timestamp);
  const pad = (value: number): string => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** 播放次数展示：0 次不显示数字，避免整列全是 0 */
export function formatPlays(count: number): string {
  return Number.isFinite(count) && count > 0 ? String(count) : '—';
}

/**
 * 只要日期（`YYYY-MM-DD`）。
 *
 * 「最近添加」那一列回答的是"哪天入库的"，带时分秒会撑爆列宽；
 * 完整时间放 `title`。
 */
export function formatDay(timestamp: number): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return '—';
  const date = new Date(timestamp);
  const pad = (value: number): string => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/**
 * 路径只留末尾几段。
 *
 * 界面上要回答的是"这是哪个目录"，不是完整路径；完整值放在 title 里。
 * 不用 CSS 的 `direction: rtl` 去截左边 —— 它会把 `/` 和 `.` 当方向中性字符
 * 重排，`../../data/demo` 会显示成 `data/demo/../`，比溢出更难读。
 */
export function shortenPath(path: string, maxSegments = 3): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  if (parts.length <= maxSegments) return path;
  return `…/${parts.slice(-maxSegments).join('/')}`;
}
