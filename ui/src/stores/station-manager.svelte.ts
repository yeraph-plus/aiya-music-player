import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { programIssueText } from '../lib/station-i18n';
import type { PlaylistSummary, StationDetail, StationProgramEdit, StationSummary } from '../lib/ipc/generated';
import { notify } from './notify.svelte';

type StationEditInput = Parameters<typeof ipc.station.create>[0];

/** 频道与歌单的数据通道；请求代次防止快速切换时迟到详情覆盖当前频道。 */
class StationManagerStore {
  stations = $state<StationSummary[]>([]);
  detail = $state<StationDetail | null>(null);
  playlists = $state<PlaylistSummary[]>([]);
  loading = $state(false);
  private generation = 0;

  async load(id: string | null): Promise<boolean> {
    const generation = ++this.generation;
    this.loading = true;
    try {
      const [stations, playlists, detail] = await Promise.all([
        ipc.station.list(), ipc.playlist.list(), id ? ipc.station.get(id) : Promise.resolve(null),
      ]);
      if (generation !== this.generation) return false;
      this.stations = stations;
      this.playlists = playlists;
      this.detail = detail;
      return true;
    } catch (err) {
      if (generation === this.generation) notify.showError(errorMessage(err));
      return false;
    } finally {
      if (generation === this.generation) this.loading = false;
    }
  }

  async save(id: string | null, input: StationEditInput): Promise<StationSummary | null> {
    try { return id ? await ipc.station.update(id,input) : await ipc.station.create(input); }
    catch (err) { notify.showError(errorMessage(err)); return null; }
  }

  async setProgram(id: string, edit: StationProgramEdit): Promise<boolean> {
    try {
      const program = await ipc.station.setProgram(id,edit);
      if (this.detail?.id === id) this.detail = {...this.detail,program};
      return true;
    } catch (err) { notify.showError(errorMessage(err)); return false; }
  }

  async apply(id: string): Promise<boolean> {
    try { await ipc.station.apply(id); return true; }
    catch (err) { await this.programError(id, err); return false; }
  }

  async setActive(id: string, active: boolean): Promise<boolean> {
    try { await ipc.station.activate(id,active); return true; }
    catch (err) { await this.programError(id, err); return false; }
  }

  private async programError(id: string, err: unknown): Promise<void> {
    try {
      const { program } = await ipc.station.get(id);
      if (program.issues.length) {
        notify.showError(program.issues.map(issue => programIssueText(issue, program.sources)).join('\n'));
        return;
      }
    } catch { /* 获取失败时保留原始错误。 */ }
    notify.showError(errorMessage(err));
  }

  async skip(id: string): Promise<void> {
    try { await ipc.station.skip(id); }
    catch (err) { notify.showError(errorMessage(err)); }
  }

  async remove(id: string): Promise<boolean> {
    try { await ipc.station.remove(id); return true; }
    catch (err) { notify.showError(errorMessage(err)); return false; }
  }
}

export const stationManager = new StationManagerStore();
