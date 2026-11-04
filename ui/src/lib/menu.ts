import { t } from './i18n.svelte';
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
  /** 选中标记（预设菜单的当前项）：组件画勾，读屏不把"✓"念进 label */
  selected?: boolean;
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
 * `multi` = 对整个选择集生效：右键的行在选择集里且不止一行。
 * 单行专属项（播放 / 下一首播放 / 查看艺人 / 查看专辑 / 在文件夹中显示）
 * 整体隐藏；批量项（加入队列 / 加入歌单 / 移除）与工具条同语义，保留。
 *
 * `self`：右键发生在哪个详情页上。专辑页的「查看专辑」指向的
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
      { label: t('lib.menu.play'), action: actions.play, disabled: !track },
      { label: t('lib.menu.playNext'), action: actions.insertNext, disabled: !track },
    );
  }
  items.push(
    { label: t('lib.menu.enqueue'), action: actions.enqueue, disabled: !track },
    { label: t('lib.menu.addToPlaylist'), action: actions.addToPlaylist, disabled: !track },
  );
  if (!multi) {
    // B4-3b 起直达艺人/专辑详情页（不再是"筛选 chip"），文案随之改为浏览语义
    items.push(
      { label: t('lib.menu.viewArtist'), action: actions.filterByArtist, disabled: !track?.artistId || self === 'artist' },
      { label: t('lib.menu.viewAlbum'), action: actions.filterByAlbum, disabled: !track?.albumId || self === 'album' },
      { label: t('lib.menu.reveal'), action: actions.reveal, disabled: !track },
    );
  }
  items.push({
    label: inPlaylist ? t('lib.menu.removeFromPlaylist') : t('lib.menu.removeFromLibrary'),
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
    { label: t('lib.menu.play'), action: actions.play },
    { label: playlist.kind === 'smart' ? t('lib.menu.editRules') : t('lib.menu.rename'), action: actions.rename },
    { label: t('lib.menu.moveUp'), action: actions.moveUp, disabled: position.index <= 0 },
    {
      label: t('lib.menu.moveDown'),
      action: actions.moveDown,
      disabled: position.index < 0 || position.index >= position.total - 1,
    },
    // 一致（曲目是真实入库行，不再是规则的投影），清空后点「刷新规则」可重新追加
    { label: t('lib.menu.clear'), action: actions.clear, disabled: playlist.trackCount === 0 },
    { label: t('lib.menu.delete'), action: actions.remove, danger: true },
  ];
}
