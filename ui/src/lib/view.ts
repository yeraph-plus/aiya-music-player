import type { TrackSource } from './query';

/**
 * 视图模型。
 *
 * "全部曲目 / 最近播放 / 常听 / 某个播放列表"共用同一套分页协议，
 * 区别只在数据来源（TrackSource）与标题。把这一层抽出来的好处是
 * 列表、分页窗口、批量选择都不需要知道自己在看什么。
 */

export type View =
  | { kind: 'library' }
  | { kind: 'recent' }
  | { kind: 'mostPlayed' }
  | { kind: 'playlist'; id: string };

/**
 * 单例视图对象。**必须是常量**：`ui.view` 是 getter，每次求值都重建对象的话，
 * 依赖它做记忆的下游（`$derived`、`TrackWindow` 的补页判断）会白算。
 */
export const LIBRARY_VIEW: View = { kind: 'library' };
export const RECENT_VIEW: View = { kind: 'recent' };
export const MOST_PLAYED_VIEW: View = { kind: 'mostPlayed' };

export function viewKey(view: View): string {
  return view.kind === 'playlist' ? `playlist:${view.id}` : view.kind;
}

export function parseViewKey(key: string | undefined | null): View {
  if (!key) return LIBRARY_VIEW;
  if (key === 'recent' || key === 'mostPlayed') return { kind: key };
  if (key.startsWith('playlist:')) {
    const id = key.slice('playlist:'.length);
    return id ? { kind: 'playlist', id } : LIBRARY_VIEW;
  }
  return LIBRARY_VIEW;
}

/** 三个内置列表视图的单例（按 kind 取，避免调用点各写一份对象字面量） */
export function builtinView(kind: 'library' | 'recent' | 'mostPlayed'): View {
  if (kind === 'recent') return RECENT_VIEW;
  if (kind === 'mostPlayed') return MOST_PLAYED_VIEW;
  return LIBRARY_VIEW;
}

export function sourceOf(view: View): TrackSource {
  if (view.kind === 'playlist') return { kind: 'playlist', playlistId: view.id };
  if (view.kind === 'recent') return { kind: 'recent' };
  if (view.kind === 'mostPlayed') return { kind: 'mostPlayed' };
  return { kind: 'library' };
}

export function viewTitle(view: View, playlistName?: string | null): string {
  switch (view.kind) {
    case 'recent':
      return '最近播放';
    case 'mostPlayed':
      return '常听';
    case 'playlist':
      return playlistName ?? '播放列表';
    default:
      return '全部曲目';
  }
}

/** 只有"全部曲目"有排序控件；其余视图的顺序是语义的一部分 */
export function viewCanSort(view: View): boolean {
  return view.kind === 'library';
}
