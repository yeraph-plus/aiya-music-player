import { errorMessage } from '../lib/errors';
import { events, ipc } from '../lib/ipc';
import type { RefreshSnapshot } from '../lib/ipc/generated/RefreshSnapshot';
import type { RefreshState } from '../lib/ipc/generated/RefreshState';
import { library } from './library.svelte';
import { list } from './list.svelte';
import { notify } from './notify.svelte';

/**
 * 扫描编排：选目录 → 扫描 → 刷新统计/歌单/列表。
 *
 * 为什么不放在 `library` store：`library` 是**最底层**的曲库 store（只依赖
 * ipc），而这里要串 4 个 store。放进去就得让 `library` import `list`，
 * 而 `list` 已经 import `library` —— 直接成环。
 *
 * 判断依据（沿用 docs/18 §8.2）：**能只用自己那层实现的留在原 store，
 * 需要编排别的 store 的往上提一层**。扫描要"扫完让列表重载"，所以在这。
 */
class ScanStore {
  /**
   * 自动刷新管线的状态（docs/33 §7）。
   *
   * 为什么不放 `library` store：拉它要 `ipc.refresh.state()`（那是 library 的
   * 地盘），但**消费**它的地方要串"重拉列表"，所以跟 `afterScan` 待在一起。
   * 只放查询结果，不放"要不要刷"的判断——判断在宿主，UI 只反映。
   */
  refresh = $state<RefreshState | null>(null);

  /** 拉一次刷新状态（诊断区打开 / 点刷新时）。失败静默：诊断区空着比报错好 */
  async loadRefreshState(): Promise<void> {
    try {
      this.refresh = await ipc.refresh.state();
    } catch (e) {
      console.warn('refresh.state failed:', errorMessage(e));
    }
  }

  /** 弹目录选择器，选中就扫。用户取消（返回 null）什么也不做 */
  async pickRootAndScan(): Promise<void> {
    notify.clearError();
    let path: string | null;
    try {
      path = await library.chooseRoot();
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    if (path) await this.start();
  }

  /** 用已有根目录重扫（启动时的「重新扫描」按钮 / 扫描完成后的手动触发） */
  async start(): Promise<void> {
    notify.clearError();
    try {
      await library.startScan();
    } catch (err) {
      notify.showError(errorMessage(err));
      return;
    }
    await this.afterScan();
  }

  /**
   * 扫描完成后的统一收尾。
   *
   * 3 个地方都要做同样的事（手动扫描、进度事件 `done`、启动引导），
   * 之前是各写一遍 —— 少写一个 `refreshPlaylists` 的症状是"扫完歌单
   * 里的歌还在、但侧栏计数不对"，很难发现。
   */
  async afterScan(): Promise<void> {
    await library.loadStats();
    await library.loadPlaylists();
    await list.reload({ restore: true });
  }

  /** 取消正在跑的扫描（后端收尾到最近检查点，已导入的保留） */
  cancel(): void {
    library.cancelScan();
  }

  /** 订阅扫描进度；`done` 时自动收尾。返回清理函数 */
  watch(): () => void {
    return library.watchScan(() => void this.afterScan());
  }

  /**
   * 订阅自动刷新终态：**有变化才重拉**（宿主已经按这个口径发事件了）。
   *
   * 串行化：一次重拉没看完又来一条时，记下 `again` 在结束后补一次，
   * 而不是并发跑两遍 `list.reload`（那会让游标分页的两个响应互相覆盖）。
   *
   * 为什么不弹提示：这条链路是"用户在外面改了东西，界面自己跟上"。
   * 每 30 秒弹一次 toast 会让人以为出了什么事。真要确认它活着，看诊断区。
   */
  watchAutoRefresh(): () => void {
    let running = false;
    let again = false;
    const reload = async (): Promise<void> => {
      if (running) {
        again = true;
        return;
      }
      running = true;
      try {
        await this.afterScan();
        await this.loadRefreshState();
      } catch (e) {
        // 重拉失败不致命：下一次刷新还会再来；用户手动操作也照旧
        console.warn('auto refresh reload failed:', errorMessage(e));
      } finally {
        running = false;
      }
      if (again) {
        again = false;
        void reload();
      }
    };

    return events.beetsImport((raw) => {
      const type = typeof raw.type === 'string' ? raw.type : '';
      if (type === 'libraryRefreshed') {
        const changed = raw.changed === true;
        this.refresh = {
          intervalSecs: this.refresh?.intervalSecs ?? 30,
          focused: this.refresh?.focused ?? true,
          last: raw as unknown as RefreshSnapshot,
        };
        if (changed) void reload();
      } else if (type === 'libraryRefreshFailed') {
        // 整条管线失败（DB 打不开之类）值得说一句：这是"库坏了"级别的信号
        notify.showError(`曲库自动刷新失败：${String(raw.message ?? '未知原因')}`);
      }
    });
  }
}

export const scan = new ScanStore();

