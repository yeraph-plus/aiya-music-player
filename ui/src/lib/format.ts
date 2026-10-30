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

/* ---------------- 临时项（不进曲库，docs/52 §6） ---------------- */

/**
 * 与 Rust 侧 `TrackRef::FILE_ID_PREFIX` / `STREAM_ID_PREFIX` **一字不差**。
 *
 * 这两条是 UI 唯一能看到的"这一项从哪来"的信息：`TrackRef.path` 是
 * `skip_serializing`（路径只在宿主内部流转，不下发 UI），而 `id` 里带着
 * 原始路径/URL。改前缀要两边一起改，否则来源那行字静默变空。
 */
export const FILE_ID_PREFIX = 'file:';
export const STREAM_ID_PREFIX = 'stream:';

/** 不在曲库里的项（拖入的本地文件 / 直播流） */
export function isTransientId(id: string | null | undefined): boolean {
  return Boolean(id && (id.startsWith(FILE_ID_PREFIX) || id.startsWith(STREAM_ID_PREFIX)));
}

/** 直播流项（队列面板/播放条据此把"时长"换成"直播"） */
export function isStreamId(id: string | null | undefined): boolean {
  return Boolean(id && id.startsWith(STREAM_ID_PREFIX));
}

/**
 * 临时项的"来源"一行字：本地文件 → 所在目录（末 2 段），直播 → 主机名。
 *
 * 为什么不是"编码 + 码率"（docs/52 §7.1 的原计划）：这两样此刻都拿不到——
 * `TrackRef.audio` 由曲库单曲查询填，临时项恒为 `null`；要把 TS 里的
 * `stream_type` / ADTS 帧率接到 UI，得给 `PlayerState` 加字段 + 重生成
 * ts-rs 绑定。用户拍板"给什么码率就播什么码率、不做额外支持"，那就别为了
 * 一个副标题开新契约。目录名和主机名是此刻**确定**的信息，也是用户真正
 * 会拿来对号入座的东西。
 *
 * 认不出来返回 `null`（调用方回落到通用文案，不要显示半截 URL）。
 */
export function transientOrigin(id: string | null | undefined): string | null {
  if (!id) return null;

  if (id.startsWith(STREAM_ID_PREFIX)) {
    const url = id.slice(STREAM_ID_PREFIX.length);
    // 本地播放列表（`file:///…/index.m3u8`）没有主机名
    if (/^file:/i.test(url)) return '本地播放列表';
    try {
      const host = new URL(url).host;
      return host || null;
    } catch {
      return null;
    }
  }

  if (id.startsWith(FILE_ID_PREFIX)) {
    const path = id.slice(FILE_ID_PREFIX.length);
    const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
    return cut > 0 ? shortenPath(path.slice(0, cut), 2) : null;
  }

  return null;
}
