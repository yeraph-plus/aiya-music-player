import type { PlaylistSummary, Track } from '@aiya/contracts';
import type { View } from './view.js';

/**
 * 右键菜单的内容定义。
 *
 * 抽出来的理由不是为了少写几行模板：菜单项里"哪一项该禁用"是业务规则
 * （没有艺人就不能按艺人筛选、播放列表第一项不能上移），
 * 放在模板里只能靠嵌套三元表达式表达，既难读也没法测。
 */

export interface MenuItem {
  label: string;
  action: () => void;
  disabled?: boolean;
  danger?: boolean;
}

export interface TrackMenuActions {
  play(): void;
  insertNext(): void;
  enqueue(): void;
  addToPlaylist(): void;
  filterByArtist(): void;
  filterByAlbum(): void;
  reveal(): void;
  /** 播放列表视图下是"从列表移除"，曲库视图下是"从曲库移除" */
  remove(): void;
}

export function trackMenuItems(
  track: Track | null,
  view: View,
  actions: TrackMenuActions,
): MenuItem[] {
  const inPlaylist = view.kind === 'playlist';
  return [
    { label: '播放', action: actions.play, disabled: !track },
    { label: '下一首播放', action: actions.insertNext, disabled: !track },
    { label: '加入播放队列', action: actions.enqueue, disabled: !track },
    { label: '添加到播放列表…', action: actions.addToPlaylist, disabled: !track },
    { label: '按此艺人筛选', action: actions.filterByArtist, disabled: !track?.artistId },
    { label: '按此专辑筛选', action: actions.filterByAlbum, disabled: !track?.albumId },
    { label: '在文件夹中显示', action: actions.reveal, disabled: !track },
    {
      label: inPlaylist ? '从播放列表移除' : '从曲库移除',
      action: actions.remove,
      disabled: !track,
      danger: true,
    },
  ];
}

export interface PlaylistMenuActions {
  play(): void;
  rename(): void;
  moveUp(): void;
  moveDown(): void;
  clear(): void;
  remove(): void;
}

export function playlistMenuItems(
  playlist: PlaylistSummary,
  position: { index: number; total: number },
  actions: PlaylistMenuActions,
): MenuItem[] {
  return [
    { label: '播放', action: actions.play },
    { label: '重命名', action: actions.rename },
    { label: '上移', action: actions.moveUp, disabled: position.index <= 0 },
    {
      label: '下移',
      action: actions.moveDown,
      disabled: position.index < 0 || position.index >= position.total - 1,
    },
    { label: '清空', action: actions.clear, disabled: playlist.trackCount === 0 },
    { label: '删除', action: actions.remove, danger: true },
  ];
}
