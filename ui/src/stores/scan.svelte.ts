import { errorMessage } from '../lib/errors';
import { library } from './library.svelte';
import { list } from './list.svelte';
import { notify } from './notify.svelte';
import { ui } from './ui.svelte';

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
    await ui.refreshPlaylists();
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
}

export const scan = new ScanStore();
