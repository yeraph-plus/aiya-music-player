import { SHORTCUT_ACTIONS, matchBinding, type KeyEventLike, type ShortcutAction } from '@aiya/contracts';

/**
 * 按键 ↔ 动作的桥接。
 *
 * 匹配逻辑在 contracts 里（主进程也要用），这里只负责两件 DOM 相关的事：
 * 把 KeyboardEvent 转成绑定字符串（改键用）、按动作顺序查表。
 */

const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'CapsLock']);

/** 把真实按键转成绑定字符串；只按下修饰键或无法表达的键返回 null */
export function bindingFromEvent(event: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;
  // '+' 与绑定语法的分隔符冲突，不支持录制
  if (event.key === '+') return null;

  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  if (event.metaKey) parts.push('Meta');

  let key = event.key;
  if (key === ' ') key = 'Space';
  else if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  return parts.join('+');
}

export function actionForEvent(
  event: KeyEventLike,
  bindings: Record<ShortcutAction, string>,
): ShortcutAction | null {
  for (const action of SHORTCUT_ACTIONS) {
    const binding = bindings[action];
    if (binding && matchBinding(event, binding)) return action;
  }
  return null;
}
