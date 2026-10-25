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
  | { kind: 'createPlaylist'; seedIds?: string[] }
  | { kind: 'renamePlaylist'; playlist: PlaylistSummary }
  | { kind: 'deletePlaylist'; playlist: PlaylistSummary }
  | { kind: 'addToPlaylist'; ids: string[] }
  | { kind: 'removeTracks'; ids: string[] }
  | null;

export interface TrackMenu {
  x: number;
  y: number;
  index: number;
  track: TrackRow | null;
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
   * 不含设置面板（那是 `ui.settingsOpen`）：这个标记被键盘用来决定
   * 要不要吞掉全局快捷键，而设置面板里还要能用 Esc 关闭，调用方自己叠。
   */
  get hasModal(): boolean {
    return this.dialog !== null || this.trackMenu !== null || this.playlistMenu !== null;
  }

  /**
   * 两个菜单**互斥**：右键曲目行之后再去右键侧栏歌单，不该两个都挂在屏幕上
   * （2026-10-25 真窗口验证时看到过两个 `.context-menu` 同时存在）。
   */
  openTrackMenu(x: number, y: number, index: number, track: TrackRow | null): void {
    this.playlistMenu = null;
    this.trackMenu = { x, y, index, track };
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
