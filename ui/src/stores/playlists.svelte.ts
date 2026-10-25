import type { PlaylistSummary } from '@aiya/contracts';
import { errorMessage } from '../lib/errors';
import { LIBRARY_VIEW } from '../lib/view';
import { list } from './list.svelte';
import { notify } from './notify.svelte';
import { selection } from './selection.svelte';
import { ui } from './ui.svelte';

/**
 * 播放列表的**组合动作**：CRUD + 刷新 + 视图重载 + 提示。
 *
 * 为什么单独一层（批次 3）：`ui` store 只有"调命令"（`createPlaylist` /
 * `renamePlaylist` …，全部抛出、不吞错），而一个用户动作往往要串 3~4 步：
 *
 *   创建 → 刷新侧栏 → 跳到新歌单           （`create`）
 *   删除 → 刷新侧栏 → 若正在看它则退回曲库   （`remove`）
 *   加歌 → 刷新侧栏 → 若正在看它则按原位置重载（`addTracks`）
 *
 * 这些串法留在 App.svelte 就是 120 行回调汤，且「是否正在看这个歌单」
 * 的判断在 5 个地方各写一遍。放到这里，视图与歌单的关系只在这一处判断。
 *
 * 错误处理分工：命令抛出 → 这里 `notify.showError`（用户显式动作，
 * 必须让他看到）；失败后**不做后续步骤**（没创建成功就别跳过去）。
 */
class PlaylistsStore {
  /** 新建。成功返回新 id（调用方可能要跳过去），失败返回 null */
  async create(name: string, seedIds?: string[]): Promise<string | null> {
    let id: string;
    try {
      id = await ui.createPlaylist(name, seedIds);
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
    await ui.refreshPlaylists();
    this.open(id);
    return id;
  }

  async rename(playlist: PlaylistSummary, name: string): Promise<void> {
    try {
      await ui.renamePlaylist(playlist.id, name);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await ui.refreshPlaylists();
  }

  /**
   * 删除。若当前正在看它，退回曲库 —— 否则用户会盯着一个不存在的歌单
   * 看空列表（`refreshPlaylists` 里也有兜底，但那里不负责重载列表）。
   *
   * **必须先把"是否正在看它"存成布尔**（2026-10-25 实测踩到）：
   * `refreshPlaylists` 内部发现当前歌单不在新列表里时，会**自己**把
   * `ui.view` 改成 `LIBRARY_VIEW`。之后再判断 `ui.view.kind === 'playlist'`
   * 恒为假 → reload 被跳过 → 侧栏已经切回「全部曲目」，主区却还停在空态，
   * 用户得手点一下侧栏才出数据。旧实现在 App.svelte 里也是这个写法
   * （`view` 是 `$derived(ui.view)`，读的同样是刷新后的值），所以这是
   * 迁移前就存在的 bug，顺手修掉。
   */
  async remove(playlist: PlaylistSummary): Promise<void> {
    const wasViewing = ui.view.kind === 'playlist' && ui.view.id === playlist.id;
    try {
      await ui.removePlaylist(playlist.id);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await ui.refreshPlaylists();
    if (wasViewing) {
      ui.view = LIBRARY_VIEW;
      await list.reload();
    }
    notify.showToast(`已删除「${playlist.name}」`);
  }

  async clear(playlist: PlaylistSummary): Promise<void> {
    try {
      await ui.clearPlaylist(playlist.id);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await ui.refreshPlaylists();
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
    let added: { added: number; skipped: number };
    try {
      added = await ui.addTracks(playlistId, ids);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await ui.refreshPlaylists();
    notify.showToast(`新增 ${added.added} 首，跳过 ${added.skipped} 首`);
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
    ui.view = { kind: 'playlist', id: playlist.id };
    await list.reload();
    await list.playAll();
  }

  /** 侧栏上下移动（`delta` = ∓1）。到边界时静默忽略 */
  async moveOrder(playlist: PlaylistSummary, delta: number): Promise<void> {
    const index = ui.playlists.findIndex((item) => item.id === playlist.id);
    if (index < 0) return;
    const target = index + delta;
    if (target < 0 || target >= ui.playlists.length) return;
    try {
      await ui.reorderPlaylists(index, target);
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /** 歌单内拖动排序 */
  async moveItem(from: number, to: number): Promise<void> {
    const view = ui.view;
    if (view.kind !== 'playlist') return;
    try {
      await ui.moveItem(view.id, from, to);
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
      await ui.removeItemAt(view.id, index);
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await ui.refreshPlaylists();
    await list.reload({ restore: true });
  }

  /** 打开歌单（切视图 + 重载 + 存状态） */
  open(id: string): void {
    list.openPlaylist(id);
  }
}

export const playlists = new PlaylistsStore();
