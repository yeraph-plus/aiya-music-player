import type { ShortcutAction } from '@aiya/contracts';
import type { KeyEventLike } from '@aiya/contracts';
import { actionForEvent } from './keys.js';

/**
 * 列表键盘语义。
 *
 * 之前这段逻辑直接写在 App.svelte 的 onkeydown 里：方向键、回车、
 * Esc、以及可配置快捷键混在一个函数里，还夹着 DOM 判断。
 * 抽成"事件 → 意图"的纯函数之后，优先级（模态 > 方向键 > Esc > 快捷键）
 * 可以被单测固定下来 —— 这类优先级一旦写错，表现是"某个快捷键突然不灵了"，
 * 极难从界面上定位。
 *
 * 只处理"非输入态"的按键；输入框里的 Esc 由调用方处理。
 */

export type KeyIntent =
  | { kind: 'move'; index: number }
  | { kind: 'play'; index: number }
  | { kind: 'clearSelection' }
  | { kind: 'closePanel' }
  | { kind: 'action'; action: ShortcutAction }
  | null;

export interface KeyContext {
  total: number;
  selectedIndex: number;
  hasSelection: boolean;
  hasPanel: boolean;
  /** 设置/对话框/右键菜单打开时，列表快捷键必须让位 */
  modalOpen: boolean;
  bindings: Record<ShortcutAction, string>;
}

export function listKeyIntent(event: KeyEventLike, ctx: KeyContext): KeyIntent {
  if (ctx.modalOpen) return null;

  if (event.key === 'ArrowDown') {
    if (ctx.total === 0) return null;
    return { kind: 'move', index: Math.min(ctx.total - 1, ctx.selectedIndex + 1) };
  }
  if (event.key === 'ArrowUp') {
    if (ctx.total === 0) return null;
    return { kind: 'move', index: Math.max(0, ctx.selectedIndex - 1) };
  }
  if (event.key === 'Enter' && ctx.selectedIndex >= 0 && !event.ctrlKey) {
    return { kind: 'play', index: ctx.selectedIndex };
  }
  if (event.key === 'Escape') {
    if (ctx.hasSelection) return { kind: 'clearSelection' };
    if (ctx.hasPanel) return { kind: 'closePanel' };
    return null;
  }

  const action = actionForEvent(event, ctx.bindings);
  return action ? { kind: 'action', action } : null;
}
