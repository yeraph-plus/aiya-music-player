import { errorMessage } from '../lib/errors';
import { t } from '../lib/i18n.svelte';
import { events, ipc } from '../lib/ipc';
import type { RefreshSnapshot } from '../lib/ipc/generated/RefreshSnapshot';
import type { RefreshState } from '../lib/ipc/generated/RefreshState';
import { library } from './library.svelte';
import { list } from './list.svelte';

/**
 * 扫描编排：选目录 → 扫描 → 刷新统计/歌单/列表。
 *
 * 为什么不放在 `library` store：`library` 是**最底层**的曲库 store（只依赖
 * ipc），而这里要串 4 个 store。放进去就得让 `library` import `list`，
 * 而 `list` 已经 import `library` —— 直接成环。
 *
 * 判断依据（沿用）：**能只用自己那层实现的留在原 store
 * 需要编排别的 store 的往上提一层**。扫描要"扫完让列表重载"，所以在这。
 */
class ScanStore {
  /**
   * 自动刷新管线的状态。
   *
   * 为什么不放 `library` store：拉它要 `ipc.refresh.state()`（那是 library 的
   * 地盘），但**消费**它的地方要串"重拉列表"，所以跟 `afterScan` 待在一起。
   * 只放查询结果，不放"要不要刷"的判断——判断在宿主，UI 只反映。
   */
  refresh = $state<RefreshState | null>(null);

  /**
   * 不再走全局 toast——错误和它的上下文（进度/上次结果）应该在同一个
   * 视线里。展示与清除都在设置页「曲库」组的曲库卡片里）。
   */
  lastError = $state<string | null>(null);

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
    this.lastError = null;
    let path: string | null;
    try {
      path = await library.chooseRoot();
    } catch (err) {
      this.lastError = errorMessage(err);
      return;
    }
    if (path) await this.start();
  }

  /**
   * 用已有根目录重扫（启动时的「重新扫描」按钮 / 扫描完成后的手动触发）。
   *
   * 收尾**不在这里做**：`library.watchScan`（boot 时订阅）在 `done` /
   * `cancelled` 两个终态事件上都会调 `afterScan`，这里再调一次就是每轮
   * 扫描收尾跑两遍（统计/歌单/列表整套重拉）。失败分支（命令 reject，
   * 未产生终态事件）本来就不该收尾——什么都没提交。
   */
  async start(): Promise<void> {
    this.lastError = null;
    try {
      await library.startScan();
    } catch (err) {
      this.lastError = errorMessage(err);
    }
  }

  /**
   * 扫描完成后的统一收尾。
   *
   * 3 个地方都要做同样的事（手动扫描、进度事件 `done`、启动引导），
   * 之前是各写一遍 —— 少写一个 `refreshPlaylists` 的症状是"扫完歌单
   * 里的歌还在、但侧栏计数不对"，很难发现。
   */
  async afterScan(): Promise<void> {
    library.invalidateGenres(); // 曲目变了，流派下拉的缓存过期
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
        const snap = readRefreshSnapshot(raw);
        // 载荷形状不对：整条丢弃，卡片保留上一次的结果，不拿 undefined 填充
        if (!snap) return;
        this.refresh = {
          intervalSecs: this.refresh?.intervalSecs ?? 30,
          focused: this.refresh?.focused ?? true,
          last: snap,
        };
        if (snap.changed) void reload();
      } else if (type === 'libraryRefreshFailed') {
        // 整条管线失败（DB 打不开之类）：状态卡片里要看得见"库坏了"级别的信号
        this.lastError = t('scan.autoRefreshFailed', {
          message: String(raw.message ?? t('scan.unknownReason')),
        });
      }
    });
  }
}

/**
 * 动态 JSON → `RefreshSnapshot`：逐字段 typeof 收口（与 ui store 的
 * `readBeetsStatus` 同一方式）。
 *
 * 载荷来自宿主的自动刷新管线，形状随版本演化；`atMs` 是判别字段：
 * 不是 number 时整包丢弃（返回 null = 状态卡片保留上一次的结果）。
 */
function readRefreshSnapshot(raw: unknown): RefreshSnapshot | null {
  if (!raw || typeof raw !== 'object') return null;
  const o = raw as Record<string, unknown>;
  if (typeof o.atMs !== 'number') return null;
  const num = (v: unknown): number => (typeof v === 'number' ? v : 0);
  return {
    trigger: typeof o.trigger === 'string' ? o.trigger : '',
    atMs: o.atMs,
    changed: o.changed === true,
    scanned: num(o.scanned),
    imported: num(o.imported),
    updated: num(o.updated),
    missing: num(o.missing),
    failed: num(o.failed),
    elapsedMs: num(o.elapsedMs),
    error: typeof o.error === 'string' ? o.error : null,
  };
}

export const scan = new ScanStore();

