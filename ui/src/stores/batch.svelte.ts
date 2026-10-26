import { errorMessage } from '../lib/errors';
import { isListPage, ui } from './ui.svelte';
import { list } from './list.svelte';
import { notify } from './notify.svelte';
import { overlay } from './overlay.svelte';
import { player } from './player.svelte';
import { playlists } from './playlists.svelte';
import { selection } from './selection.svelte';

/** 首页三个预览区块（与 HomeView 的 `<TrackTable>` 一一对应） */
export type HomeSection = 'recent' | 'frequent' | 'added';

const EMPTY: ReadonlySet<string> = new Set();

/**
 * 批量选择的统一入口（B4-2b）。
 *
 * **批量条全应用只有一个**（AppShell 渲染），但选择集有两个来源：
 * - 列表页（曲库/歌单…）：`selection` store 存**视图下标**，操作前要交给
 *   主进程按当前查询换算成 ID（虚拟列表拿不到全量 ID，ADR-020）；
 * - 首页三段：`TrackTable` 直接回传**曲目 ID**（三段各自独立查询，
 *   下标跨段无意义）。
 *
 * 之前是两份几乎相同的动作实现（`list.batchPlay` vs HomeView 本地
 * `batchPlay`…）+ 两个批量条渲染点，改文案/加动作要记得改两处。
 * 现在动作只有这一份，"当前选中什么"由 `active`/`count`/`ids()` 按路由
 * 判别来源。同一时刻至多一个来源生效：跨页导航时另一来源自然失活
 * （`active` 只认当前页），残留集合在下次操作时被覆盖。
 */
class BatchStore {
  /** 首页区块选择：段名 + 选中 ID */
  section = $state<{ name: HomeSection; ids: Set<string> } | null>(null);

  /** 批量条是否该渲染（AppShell 唯一读取点） */
  get active(): boolean {
    if (isListPage(ui.page)) return selection.size > 0;
    if (ui.page === 'home') return (this.section?.ids.size ?? 0) > 0;
    return false;
  }

  /** 批量条上显示的数量 */
  get count(): number {
    if (isListPage(ui.page)) return selection.size;
    return this.section?.ids.size ?? 0;
  }

  /** 首页各段的选择集（没选中的段给同一个空集，引用稳定） */
  selectedIn(name: HomeSection): ReadonlySet<string> {
    return this.section?.name === name ? this.section.ids : EMPTY;
  }

  /** `TrackTable` 的 `onchange`：换段自动清上一段（批量条只有一个，不能代表两段） */
  setSection(name: HomeSection, ids: Set<string>): void {
    this.section = ids.size > 0 ? { name, ids } : null;
  }

  /** 当前生效来源解析成曲目 ID：列表页走后端换算，首页用现成集合 */
  private async ids(): Promise<string[]> {
    if (isListPage(ui.page) && selection.size > 0) return list.selectionIds();
    return [...(this.section?.ids ?? [])];
  }

  /** 动作完成后清掉**生效中的那个**来源 */
  private clearActive(): void {
    if (isListPage(ui.page) && selection.size > 0) selection.clear();
    else this.section = null;
  }

  /** 选中项替换当前队列并从第一首起播 */
  async play(): Promise<void> {
    const ids = await this.ids();
    if (ids.length === 0) return;
    try {
      await player.playTracks(ids, 0);
      this.clearActive();
    } catch (error) {
      notify.showError(errorMessage(error));
    }
  }

  /** 选中项追加到队列尾部 */
  async enqueue(): Promise<void> {
    const ids = await this.ids();
    if (ids.length === 0) return;
    try {
      await player.enqueue(ids);
      notify.showToast(`已加入队列 ${ids.length} 首`);
      this.clearActive();
    } catch (error) {
      notify.showError(errorMessage(error));
    }
  }

  /** 选中项加入歌单（不关批量条之前的确认由 `PlaylistPicker` 承担） */
  async addTo(playlistId: string): Promise<void> {
    const ids = await this.ids();
    if (ids.length === 0) return;
    await playlists.addTracks(playlistId, ids);
    this.clearActive();
  }

  /** 从曲库移除（走确认对话框；只删索引不动磁盘文件） */
  async remove(): Promise<void> {
    const ids = await this.ids();
    if (ids.length === 0) return;
    overlay.dialog = { kind: 'removeTracks', ids };
  }

  /** 批量条 ✕：只清选择，不动数据 */
  clear(): void {
    this.clearActive();
  }
}

export const batch = new BatchStore();
