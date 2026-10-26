import type { PlaylistSummary } from './ipc/generated/PlaylistSummary';
import type { TrackRow } from './ipc/generated/TrackRow';
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

/**
 * `multi` = 对整个选择集生效（docs/27）：右键的行在选择集里且不止一行。
 * 单行专属项（播放 / 下一首播放 / 查看艺人 / 查看专辑 / 在文件夹中显示）
 * 整体隐藏；批量项（加入队列 / 加入歌单 / 移除）与工具条同语义，保留。
 *
 * `self`（docs/28）：右键发生在哪个详情页上。专辑页的「查看专辑」指向的
 * 就是当前页（nav 视为刷新、白丢滚动位置），禁掉——列表列链接同一条规则。
 */
export function trackMenuItems(
  track: TrackRow | null,
  view: View,
  actions: TrackMenuActions,
  multi = false,
  self: 'album' | 'artist' | null = null,
): MenuItem[] {
  const inPlaylist = view.kind === 'playlist';
  const items: MenuItem[] = [];
  if (!multi) {
    items.push(
      { label: '播放', action: actions.play, disabled: !track },
      { label: '下一首播放', action: actions.insertNext, disabled: !track },
    );
  }
  items.push(
    { label: '加入播放队列', action: actions.enqueue, disabled: !track },
    { label: '添加到播放列表…', action: actions.addToPlaylist, disabled: !track },
  );
  if (!multi) {
    // B4-3b 起直达艺人/专辑详情页（不再是"筛选 chip"），文案随之改为浏览语义
    items.push(
      { label: '查看艺人', action: actions.filterByArtist, disabled: !track?.artistId || self === 'artist' },
      { label: '查看专辑', action: actions.filterByAlbum, disabled: !track?.albumId || self === 'album' },
      { label: '在文件夹中显示', action: actions.reveal, disabled: !track },
    );
  }
  items.push({
    label: inPlaylist ? '从播放列表移除' : '从曲库移除',
    action: actions.remove,
    disabled: !track,
    danger: true,
  });
  return items;
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
