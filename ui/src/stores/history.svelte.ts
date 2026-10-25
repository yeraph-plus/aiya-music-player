import { errorMessage } from '../lib/errors';
import { library } from './library.svelte';
import { list } from './list.svelte';
import { notify } from './notify.svelte';
import { ui } from './ui.svelte';

/**
 * 播放历史编排。
 *
 * 现在只有一个写操作（清空），读操作走曲库查询（`view.kind === 'recent' |
 * 'mostPlayed'` 时 `source` 不同，共用 `library.list`）。所以这里**不持有
 * 历史数据**，只放"清空之后要连带刷新什么"。
 *
 * 单独建文件而不是塞进 `library`：`clear` 要 reload 列表、要刷新诊断，
 * 而 `library` 是被 `list` 依赖的底层 store（塞进去就成环）。
 */
class HistoryStore {
  /**
   * 清空播放历史。
   *
   * 副作用比看起来多：`history` 表被清空后，**最近/常听两个视图的查询结果
   * 会变空**，侧栏统计（`library_stats` 里的 total_duration 不受影响，但
   * 设置页的诊断块里有播放计数）也要重取。少做一步的症状是"点了清空，
   * 切回去还有数据"（下次进视图会刷新，所以这种 bug 只在同屏复现）。
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
    const view = ui.view;
    if (view.kind === 'recent' || view.kind === 'mostPlayed') await list.reload();
    // 设置页开着：诊断块里的历史相关数字要跟着变
    if (ui.settingsOpen) await ui.refreshDiagnostics();
  }
}

export const history = new HistoryStore();
