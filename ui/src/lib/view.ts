import type { TrackSource } from './query';

/**
 * 视图模型。
 *
 * "全部曲目 / 某个播放列表"共用同一套分页协议，区别只在数据来源
 * （TrackSource）与标题。把这一层抽出来的好处是列表、分页窗口、批量选择
 * 都不需要知道自己在看什么。
 *
 * 2026-10-26 docs/28：`recent` / `mostPlayed` 两个视图**退役**——单页从侧栏
 * 撤掉（常听收进推荐页 top-15，最近播放被筛选器取代），但两个 TrackSource
 * 查询本身保留（推荐页的两段预览还走它们，见 `lib/ipc/client.ts`）。
 * View（路由层概念）与 TrackSource（查询层概念）从此不再一一对应。
 */

export type View =
  | { kind: 'library' }
  | { kind: 'playlist'; id: string };

/**
 * 单例视图对象。**必须是常量**：`ui.view` 是 getter，每次求值都重建对象的话，
 * 依赖它做记忆的下游（`$derived`、`TrackWindow` 的补页判断）会白算。
 */
export const LIBRARY_VIEW: View = { kind: 'library' };

export function viewKey(view: View): string {
  return view.kind === 'playlist' ? `playlist:${view.id}` : view.kind;
}

export function parseViewKey(key: string | undefined | null): View {
  if (!key) return LIBRARY_VIEW;
  if (key.startsWith('playlist:')) {
    const id = key.slice('playlist:'.length);
    return id ? { kind: 'playlist', id } : LIBRARY_VIEW;
  }
  // 旧文件里的 'recent' / 'mostPlayed' 也落到这里：两个单页已退役，
  // 恢复语义是"回到曲库"，而不是报错或白屏
  return LIBRARY_VIEW;
}

export function sourceOf(view: View): TrackSource {
  if (view.kind === 'playlist') return { kind: 'playlist', playlistId: view.id };
  return { kind: 'library' };
}
