import type { PlaylistSummary } from '../lib/ipc';
import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { LIBRARY_VIEW } from '../lib/view';
import { library } from './library.svelte';
import { list } from './list.svelte';
import { nav } from './nav.svelte';
import { notify } from './notify.svelte';
import { selection } from './selection.svelte';
import { ui } from './ui.svelte';

/**
 * 播放列表的**组合动作**：命令 + 刷新 + 视图重载 + 提示。
 *
 * 这一层只做编排，不持有数据：摘要在 `library.playlists`（曲目变动后
 * `list`/`scan` 也要刷它，放这里会成环，理由见 library 里那段注释）。
 *
 * 一个用户动作往往要串 3~4 步：
 *
 *   创建 → 刷新侧栏 → 跳到新歌单           （`create`）
 *   删除 → 刷新侧栏 → 若正在看它则退回曲库   （`remove`）
 *   加歌 → 刷新侧栏 → 若正在看它则按原位置重载（`addTracks`）
 *
 * 这些串法留在调用方就是回调汤，且「是否正在看这个歌单」的判断会散在 5 个
 * 地方。放到这里，视图与歌单的关系只在这一处判断。
 *
 * 错误处理分工：命令抛出（`ipc` 层不做兜底）→ 这里 `notify.showError`
 * （用户显式动作，必须让他看到）；失败后**不做后续步骤**（没创建成功就别跳过去）。
 */
class PlaylistsStore {
  /** 歌单摘要（只读视图；数据真身在 library，见类注释） */
  get items(): PlaylistSummary[] {
    return library.playlists;
  }

  byId(id: string): PlaylistSummary | null {
    return library.playlists.find((item) => item.id === id) ?? null;
  }

  /**
   * 重拉 + 兜底：当前正在看的歌单若已不存在，把视图退回曲库。
   *
   * 兜底放在这里而不是 `library.loadPlaylists`：它要写 `ui.view`，
   * 而 library 是最底层（只依赖 ipc），不能反向依赖 ui。
   */
  async refresh(): Promise<void> {
    await library.loadPlaylists();
    const view = ui.view;
    // setView 而不是 enterList：这里只是"当前歌单没了"的兜底，
    // 不该顺手清掉用户的搜索词与筛选（拆页前直接写 ui.view 恰好绕过了清理）
    if (view.kind === 'playlist' && !this.byId(view.id)) ui.setView(LIBRARY_VIEW);
  }

  /** 新建。成功返回新 id（调用方可能要跳过去），失败返回 null */
  async create(name: string, seedIds?: string[]): Promise<string | null> {
    let id: string;
    try {
      id = (await ipc.playlist.create(name, seedIds)).id;
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
    await this.refresh();
    this.open(id);
    return id;
  }

  async rename(playlist: PlaylistSummary, name: string): Promise<void> {
    try {
      await ipc.playlist.rename(playlist.id, name);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.refresh();
  }

  /**
   * 删除。若当前正在看它，退回曲库 —— 否则用户会盯着一个不存在的歌单
   * 看空列表（`refresh` 里也有兜底，但那里不负责重载列表）。
   *
   * **必须先把"是否正在看它"存成布尔**（2026-10-25 实测踩到）：
   * `refresh` 内部发现当前歌单不在新列表里时会自己把 `ui.view` 改成
   * `LIBRARY_VIEW`。之后再判断 `ui.view.kind === 'playlist'` 恒为假 →
   * reload 被跳过 → 侧栏已经切回「全部曲目」，主区却还停在空态。
   */
  async remove(playlist: PlaylistSummary): Promise<void> {
    const wasViewing = ui.view.kind === 'playlist' && ui.view.id === playlist.id;
    try {
      await ipc.playlist.remove(playlist.id);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.refresh();
    if (wasViewing) {
      ui.setView(LIBRARY_VIEW);
      await list.reload();
    }
    notify.showToast(`已删除「${playlist.name}」`);
  }

  async clear(playlist: PlaylistSummary): Promise<void> {
    try {
      await ipc.playlist.clear(playlist.id);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.refresh();
    const view = ui.view;
    if (view.kind === 'playlist' && view.id === playlist.id) await list.reload();
    notify.showToast('已清空播放列表');
  }

  /**
   * 加歌。返回后清空选择集（用户预期"加完就结束"）。
   * `added/skipped` 必须原样告诉用户：重复的歌被静默跳过会让人以为没加上。
   */
  async addTracks(playlistId: string, ids: string[]): Promise<void> {
    if (ids.length === 0) return;
    let added: number;
    let skipped: number;
    try {
      const result = await ipc.playlist.addTracks(playlistId, ids);
      added = result.added;
      skipped = result.skipped;
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.refresh();
    notify.showToast(`新增 ${added} 首，跳过 ${skipped} 首`);
    selection.clear();
    const view = ui.view;
    // 正在看这个歌单：按原滚动位置重载（否则新加的歌在列表底部，
    // 而用户会被弹回顶部——加一首歌不该丢阅读位置）
    if (view.kind === 'playlist' && view.id === playlistId) await list.reload({ restore: true });
  }

  /**
   * 批量条「加入播放列表」：先把选中集换成 ID 再加。
   * 选择集为空时静默返回（批量条本身在没有选中时不显示，这里是兜底）。
   */
  async addSelection(playlistId: string): Promise<void> {
    const ids = await list.selectionIds();
    if (ids.length === 0) return;
    await this.addTracks(playlistId, ids);
  }

  /** 侧栏右键「播放」：先切到该歌单再整列表起播 */
  async play(playlist: PlaylistSummary): Promise<void> {
    // 不用 this.open()：那里面 reload 是 fire-and-forget，而这里必须先
    // 等列表换成该歌单的内容，否则 playAll 播的是上一个视图的歌
    ui.setView({ kind: 'playlist', id: playlist.id });
    await list.reload();
    await list.playAll();
  }

  /** 侧栏上下移动（`delta` = ∓1）。到边界时静默忽略 */
  async moveOrder(playlist: PlaylistSummary, delta: number): Promise<void> {
    const index = library.playlists.findIndex((item) => item.id === playlist.id);
    if (index < 0) return;
    const target = index + delta;
    if (target < 0 || target >= library.playlists.length) return;
    try {
      await library.reorderPlaylists(index, target);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /** 歌单内拖动排序 */
  async moveItem(from: number, to: number): Promise<void> {
    const view = ui.view;
    if (view.kind !== 'playlist') return;
    try {
      await ipc.playlist.move(view.id, from, to);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await list.reload({ restore: true });
  }

  /** 歌单内删一首（右键「从歌单移除」） */
  async removeItemAt(index: number): Promise<void> {
    const view = ui.view;
    if (view.kind !== 'playlist' || index < 0) return;
    try {
      await ipc.playlist.removeAt(view.id, index);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.refresh();
    await list.reload({ restore: true });
  }

  /** 打开歌单（切视图 + 重载 + 存状态） */
  open(id: string): void {
    nav.go({ kind: 'playlist', id });
  }
}

export const playlists = new PlaylistsStore();
