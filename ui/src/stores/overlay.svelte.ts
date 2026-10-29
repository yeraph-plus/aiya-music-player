import type { PlaylistSummary, TrackRow } from '../lib/ipc';

/**
 * 浮层状态：右键菜单 + 模态对话框。
 *
 * 为什么进 store 而不是留在 App：键盘处理（`lib/keyboard.ts`）需要判断
 * 「有没有模态开着」（开着时不该触发空格播放），而键盘处理不能再回头
 * 依赖 App 的局部变量 —— 那就是把待拆的 App 又粘回去。
 *
 * **这里只存"哪个浮层开着"，不存业务数据**：对话框要的 `ids` / `playlist`
 * 是动作的入参快照，跟着对话框一起生死，放这里最自然。
 */
export type Dialog =
  /**
   * 歌单创建/编辑**同一个模态框**（2026-10-28 拍板）：`playlist` 缺省 = 创建，
   * 给了 = 编辑（手动歌单改名称、动态歌单改名称+规则）。
   * `seedIds` 是"新建并加入所选曲目"的入参快照。
   */
  | { kind: 'playlistEdit'; playlist?: PlaylistSummary; seedIds?: string[] }
  | { kind: 'deletePlaylist'; playlist: PlaylistSummary }
  | { kind: 'addToPlaylist'; ids: string[] }
  | { kind: 'removeTracks'; ids: string[] }
  /** 列表工具条的筛选器模态框（docs/27）；草稿状态在 OverlayHost 本地 */
  | { kind: 'trackFilter' }
  | null;

export interface TrackMenu {
  x: number;
  y: number;
  index: number;
  track: TrackRow | null;
  /**
   * 多选语义（docs/27）：右键的行在当前选择集里且选择集 >1 行时为真——
   * 菜单隐藏「播放/下一首播放/在文件夹中显示/查看艺人/查看专辑」这类
   * 单行专属项，批量项对整个选择集生效。
   */
  multi: boolean;
}

export interface PlaylistMenu {
  x: number;
  y: number;
  playlist: PlaylistSummary;
}

class OverlayStore {
  trackMenu = $state<TrackMenu | null>(null);
  playlistMenu = $state<PlaylistMenu | null>(null);
  dialog = $state<Dialog>(null);

  /**
   * 有任何模态/菜单开着吗。
   *
   * 不含设置页：设置已经是**一级页面**而不是浮层，进设置页不该吞掉全局
   * 快捷键（只有"正在录制快捷键"时才让路，那个标志在 `stores/capture`）。
   */
  get hasModal(): boolean {
    return this.dialog !== null || this.trackMenu !== null || this.playlistMenu !== null;
  }

  /**
   * 两个菜单**互斥**：右键曲目行之后再去右键侧栏歌单，不该两个都挂在屏幕上
   * （2026-10-25 真窗口验证时看到过两个 `.context-menu` 同时存在）。
   */
  openTrackMenu(
    x: number,
    y: number,
    index: number,
    track: TrackRow | null,
    multi = false,
  ): void {
    this.playlistMenu = null;
    this.trackMenu = { x, y, index, track, multi };
  }

  openPlaylistMenu(x: number, y: number, playlist: PlaylistSummary): void {
    this.trackMenu = null;
    this.playlistMenu = { x, y, playlist };
  }

  closeMenus(): void {
    this.trackMenu = null;
    this.playlistMenu = null;
  }

  closeDialog(): void {
    this.dialog = null;
  }
}

export const overlay = new OverlayStore();
