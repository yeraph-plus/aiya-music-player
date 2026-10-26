/**
 * 快捷键的清单、默认键位与解析/匹配逻辑。
 *
 * 2026-10-25 批次 4：合并自已删除的 `@aiya/contracts`（原来的 `shortcuts.ts`
 * 解码 + `domain.ts` 的常量），三块放在一起是因为"改一个键位"经常同时动
 * 清单、默认值和展示文案。
 *
 * 绑定字符串格式：`Ctrl+Shift+Q`、`Space`、`ArrowLeft`、`Ctrl+,`
 * 修饰键顺序固定为 Ctrl → Alt → Shift → Meta，比较前统一规范化，
 * 所以 `Shift+Ctrl+Q` 与 `Ctrl+Shift+Q` 视为同一个键位。
 *
 * 纯函数、不碰 DOM —— 渲染层拿它做按键匹配，设置页拿它做冲突检测。
 */

/* ---------------- 动作清单与默认键位 ---------------- */

export const SHORTCUT_ACTIONS = [
  'playPause',
  'next',
  'prev',
  // 快退在前：设置页两列排布，读起来才是"快退 → 快进"
  'seekBackward',
  'seekForward',
  'volumeUp',
  'volumeDown',
  // 倍速紧挨音量：设置页两列排布读起来是"音量组 → 速率组 → 静音"
  'rateUp',
  'rateDown',
  'mute',
  'focusSearch',
  'toggleQueue',
  'toggleLyrics',
  'toggleShuffle',
  'cycleRepeat',
  'selectAll',
  'openSettings',
] as const;

export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];

/**
 * 默认键位。改键只存覆盖项（`AppSettings.shortcuts`），
 * 这样后续调整默认值不会把用户自定义过的键位冲掉。
 */
export const DEFAULT_SHORTCUTS: Record<ShortcutAction, string> = {
  playPause: 'Space',
  next: 'Shift+ArrowRight',
  prev: 'Shift+ArrowLeft',
  seekForward: 'ArrowRight',
  seekBackward: 'ArrowLeft',
  volumeUp: 'Ctrl+ArrowUp',
  volumeDown: 'Ctrl+ArrowDown',
  rateUp: ']',
  rateDown: '[',
  mute: 'M',
  focusSearch: '/',
  toggleQueue: 'Ctrl+Shift+Q',
  toggleLyrics: 'Ctrl+Shift+L',
  toggleShuffle: 'S',
  cycleRepeat: 'R',
  selectAll: 'Ctrl+A',
  openSettings: 'Ctrl+,',
};

/** 覆盖项合并到默认键位；空字符串表示用户主动禁用该动作 */
export function resolveShortcutBindings(
  overrides: Partial<Record<ShortcutAction, string>> = {},
): Record<ShortcutAction, string> {
  const out = { ...DEFAULT_SHORTCUTS };
  for (const action of SHORTCUT_ACTIONS) {
    const value = overrides[action];
    if (typeof value === 'string') out[action] = value;
  }
  return out;
}

export const SHORTCUT_LABELS: Record<ShortcutAction, string> = {
  playPause: '播放 / 暂停',
  next: '下一首',
  prev: '上一首',
  seekForward: '快进 5 秒',
  seekBackward: '快退 5 秒',
  volumeUp: '音量 +5%',
  volumeDown: '音量 -5%',
  rateUp: '加速 0.1×',
  rateDown: '减速 0.1×',
  mute: '静音切换',
  focusSearch: '聚焦搜索框',
  toggleQueue: '播放队列面板',
  toggleLyrics: '歌词面板',
  toggleShuffle: '随机播放切换',
  cycleRepeat: '播放模式轮转（列表循环/单曲循环/随机/顺序）',
  selectAll: '全选当前视图',
  openSettings: '打开设置',
};

/* ---------------- 解析与匹配 ---------------- */

export interface KeyEventLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const MODIFIERS = ['Ctrl', 'Alt', 'Shift', 'Meta'] as const;
type Modifier = (typeof MODIFIERS)[number];

const KEY_ALIASES: Record<string, string> = {
  ' ': 'Space',
  spacebar: 'Space',
  space: 'Space',
  esc: 'Escape',
  escape: 'Escape',
  del: 'Delete',
  delete: 'Delete',
  return: 'Enter',
  enter: 'Enter',
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  plus: '+',
};

function normalizeKey(raw: string): string {
  // 空格键的 key 就是 ' '，trim 之后会变成空串，必须先特判
  if (raw === ' ') return 'Space';
  const trimmed = raw.trim();
  if (!trimmed) return '';
  const alias = KEY_ALIASES[trimmed.toLowerCase()];
  if (alias) return alias;
  if (trimmed.length === 1) return trimmed.toUpperCase();
  // ArrowUp / Backspace / PageDown…：首字母大写即可稳定比较
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1).toLowerCase();
}

function modifierOf(token: string): Modifier | null {
  const lower = token.toLowerCase();
  if (lower === 'ctrl' || lower === 'control' || lower === 'cmdorctrl') return 'Ctrl';
  if (lower === 'alt' || lower === 'option') return 'Alt';
  if (lower === 'shift') return 'Shift';
  if (lower === 'meta' || lower === 'cmd' || lower === 'command' || lower === 'super') {
    return 'Meta';
  }
  return null;
}

export interface ParsedBinding {
  key: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export function parseBinding(binding: string): ParsedBinding | null {
  const parts = String(binding ?? '')
    .split('+')
    .map((part) => part.trim())
    .filter((part) => part.length > 0);

  // 'Ctrl+,' → split('+') 会得到 ['Ctrl', ',']，这是期望行为；
  // 但 '+' 键本身写作 'plus' 或 'Ctrl++' 不好解析，这里不支持，属于可接受取舍。
  if (parts.length === 0) return null;

  const parsed: ParsedBinding = { key: '', ctrl: false, alt: false, shift: false, meta: false };
  for (const part of parts) {
    const modifier = modifierOf(part);
    if (modifier === 'Ctrl') parsed.ctrl = true;
    else if (modifier === 'Alt') parsed.alt = true;
    else if (modifier === 'Shift') parsed.shift = true;
    else if (modifier === 'Meta') parsed.meta = true;
    else if (!parsed.key) parsed.key = normalizeKey(part);
  }

  return parsed.key ? parsed : null;
}

export function normalizeBinding(binding: string): string {
  const parsed = parseBinding(binding);
  if (!parsed) return '';
  const parts: string[] = [];
  if (parsed.ctrl) parts.push('Ctrl');
  if (parsed.alt) parts.push('Alt');
  if (parsed.shift) parts.push('Shift');
  if (parsed.meta) parts.push('Meta');
  parts.push(parsed.key);
  return parts.join('+');
}

/** 事件是否命中绑定；空绑定视为未设置，永不命中 */
export function matchBinding(event: KeyEventLike, binding: string): boolean {
  const parsed = parseBinding(binding);
  if (!parsed) return false;
  if (event.key === undefined) return false;

  // 只按下了修饰键时 key 是 'Control' / 'Shift'，不该命中任何绑定的主键
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) return false;

  const key = normalizeKey(event.key);
  if (key !== parsed.key) return false;

  return (
    event.ctrlKey === parsed.ctrl &&
    event.altKey === parsed.alt &&
    event.shiftKey === parsed.shift &&
    event.metaKey === parsed.meta
  );
}

/**
 * 冲突检测：同一个键位绑定了多个动作。
 * 返回的 actions 保持传入顺序，便于 UI 稳定展示。
 */
export function findConflicts<T extends string>(
  bindings: Record<T, string>,
): Array<{ binding: string; actions: T[] }> {
  const byBinding = new Map<string, T[]>();
  for (const [action, raw] of Object.entries(bindings) as Array<[T, string]>) {
    const binding = normalizeBinding(raw);
    if (!binding) continue;
    const list = byBinding.get(binding);
    if (list) list.push(action);
    else byBinding.set(binding, [action]);
  }

  const conflicts: Array<{ binding: string; actions: T[] }> = [];
  for (const [binding, actions] of byBinding) {
    if (actions.length > 1) conflicts.push({ binding, actions });
  }
  return conflicts;
}

/**
 * 展示用：把 'Ctrl+Shift+Q' 排成 'Ctrl + Shift + Q'。
 *
 * 旧注释说排成 `Ctrl ⇧ Q` 这类紧凑形式，实际实现是全写 + " + " 分隔
 * （2026-10-25 核对后改注释，代码行为未变 —— 设置页两列布局宽度足够）。
 */
export function formatBinding(binding: string): string {
  const parsed = parseBinding(binding);
  if (!parsed) return '未设置';
  const parts: string[] = [];
  if (parsed.ctrl) parts.push('Ctrl');
  if (parsed.alt) parts.push('Alt');
  if (parsed.shift) parts.push('Shift');
  if (parsed.meta) parts.push('Meta');
  parts.push(parsed.key === 'Space' ? '空格' : parsed.key);
  return parts.join(' + ');
}
