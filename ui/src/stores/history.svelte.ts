import { errorMessage } from '../lib/errors';
import { library } from './library.svelte';
import { notify } from './notify.svelte';
import { ui } from './ui.svelte';

/**
 * 播放历史编排。
 *
 * 只有一个写操作（清空）。读操作不走这里：最近播放/常听作为**查询来源**
 * （`lib/ipc/client.ts` 的 TrackSource）由推荐页直接拉，docs/28 起两个
 * 单页退役后这里也不再把"清空"和某个列表视图的刷新绑在一起。
 *
 * 单独建文件而不是塞进 `library`：`clear` 要刷新统计与诊断，
 * 而 `library` 是被 `list` 依赖的底层 store（塞进去就成环）。
 */
class HistoryStore {
  /**
   * 清空播放历史。
   *
   * 副作用比看起来多：`history` 表被清空后，推荐页的最近播放/常听两段
   * 的数据源会变空，侧栏统计与设置页诊断块里的播放计数也要重取。
   * 少做一步的症状是"点了清空，切回去还有数据"。
   */
  async clear(): Promise<void> {
    try {
      await library.clearHistory();
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    notify.showToast('播放历史已清空');
    await library.loadStats();
    // 设置页开着：诊断块里的历史相关数字要跟着变
    if (ui.page === 'settings') await ui.refreshDiagnostics();
  }
}

export const history = new HistoryStore();
