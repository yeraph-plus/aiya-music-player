import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import type {
  StationDetail,
  StationPlaylist,
  StationPlaylistEdit,
  StationSchedule,
  StationScheduleEdit,
  StationSummary,
  TrackPage,
} from '../lib/ipc/generated';
import { notify } from './notify.svelte';

/** 创建/更新载荷：从 client 的 station 参数推导（与调用点保持同源） */
type StationEditInput = Parameters<typeof ipc.station.create>[0];

/**
 * 推流频道的命令与数据通道：state（stations/detail）在这里，组件只读；
 * 命令失败统一 `notify.showError` 并返回 null/false，成功路径的提示与
 * 界面流转留在组件。曲库搜索是选曲对话框的独立分页查询，不进曲库页的
 * library store 状态，随本 store 走。
 */
class StationManagerStore {
  /** 频道摘要列表（load 后填充） */
  stations = $state<StationSummary[]>([]);
  /** 当前打开的频道详情（load / 整表提交成功后填充） */
  detail = $state<StationDetail | null>(null);

  /**
   * 拉列表；selectedId 给了就一并刷该频道详情。频道已消失返回 false
   * （调用方据此清选中、退回列表态），命令失败也返回 false。
   */
  async load(selectedId: string | null): Promise<boolean> {
    let list: StationSummary[];
    try {
      list = await ipc.station.list();
    } catch (err) {
      notify.showError(errorMessage(err));
      return false;
    }
    this.stations = list;
    if (!selectedId) return true;
    const still = list.find((s) => s.id === selectedId);
    if (!still) return false;
    try {
      this.detail = await ipc.station.get(selectedId);
    } catch (err) {
      notify.showError(errorMessage(err));
      return false;
    }
    return true;
  }

  /** 新建（editingId = null）或保存元数据。成功返回摘要，失败 null。 */
  async save(editingId: string | null, input: StationEditInput): Promise<StationSummary | null> {
    try {
      return editingId ? await ipc.station.update(editingId, input) : await ipc.station.create(input);
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
  }

  /** 开播/停播。 */
  async setActive(stationId: string, active: boolean): Promise<boolean> {
    try {
      await ipc.station.activate(stationId, active);
      return true;
    } catch (err) {
      notify.showError(errorMessage(err));
      return false;
    }
  }

  /** 跳过当前曲目。 */
  async skip(stationId: string): Promise<boolean> {
    try {
      await ipc.station.skip(stationId);
      return true;
    } catch (err) {
      notify.showError(errorMessage(err));
      return false;
    }
  }

  /** 删除频道。 */
  async remove(stationId: string): Promise<boolean> {
    try {
      await ipc.station.remove(stationId);
      return true;
    } catch (err) {
      notify.showError(errorMessage(err));
      return false;
    }
  }

  /**
   * 整表提交歌单（server 事务全删全插）。成功后用返回的全量就地更新
   * detail 并返回它，失败 null。
   */
  async setPlaylists(stationId: string, list: StationPlaylistEdit[]): Promise<StationPlaylist[] | null> {
    try {
      const { playlists } = await ipc.station.setPlaylists(stationId, list);
      if (this.detail?.id === stationId) this.detail = { ...this.detail, playlists };
      return playlists;
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
  }

  /** 整表提交节目表，语义同 setPlaylists。 */
  async setSchedules(stationId: string, list: StationScheduleEdit[]): Promise<StationSchedule[] | null> {
    try {
      const { schedules } = await ipc.station.setSchedules(stationId, list);
      if (this.detail?.id === stationId) this.detail = { ...this.detail, schedules };
      return schedules;
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
  }

  /** 曲库分页搜索（选曲对话框用；offset/limit 由调用方管理翻页游标）。 */
  async searchTracks(search: string, offset: number, limit: number): Promise<TrackPage | null> {
    try {
      return await ipc.library.list({ search, offset, limit });
    } catch (err) {
      notify.showError(errorMessage(err));
      return null;
    }
  }
}

export const stationManager = new StationManagerStore();
