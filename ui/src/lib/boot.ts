import { errorMessage } from './errors';
import type { PlayerState } from './ipc/generated/PlayerState';
import { library } from '../stores/library.svelte';
import { list } from '../stores/list.svelte';
import { notify } from '../stores/notify.svelte';
import { playlists } from '../stores/playlists.svelte';
import { scan } from '../stores/scan.svelte';
import { player } from '../stores/player.svelte';
import { ui } from '../stores/ui.svelte';

/** 启动引导的整体时限：超过即放行主界面（IPC 挂起不该把人关在启动屏里） */
const BOOT_TIMEOUT_MS = 8000;

/**
 * 启动引导。
 *
 * 从 `App.svelte` 的 `onMount` 搬出来（批次 3）：这段是**一次性编排**
 * （订阅 → 并发拉取 → 首屏 → 对齐播放器快照），一个组件生命周期方法里
 * 塞 60 行 async 会盖住 App 真正该做的事（装配）。
 *
 * **签名刻意做成"同步返回清理函数 + 内部异步"**：`onMount` 必须在返回的
 * 那一拍就拿到清理函数。如果做成 `async function boot(): Promise<() => void>`，
 * 组件在引导完成前卸载就会泄漏订阅（老代码没错，因为它是手写在一个
 * 同步 `onMount` 里先 push 再 async）。
 */
export function boot(options: { onState: (state: PlayerState) => void }): () => void {
  const cleanups: Array<() => void> = [];

  // player:state 事件是否已到过。晚到的 fetchState 结果不能盖回旧状态。
  let stateEventSeen = false;

  cleanups.push(
    player.init({
      onState: (next) => {
        stateEventSeen = true;
        options.onState(next);
      },
    }),
  );

  // 扫描进度：`done` 时由 scan store 统一收尾（统计/歌单/列表）
  cleanups.push(scan.watch());

  // 自动刷新的终态（docs/33 §7）：有变化就重拉列表。
  // 与手动扫描共用同一条收尾路径 —— 两条链路的"扫完之后该做什么"必须一致，
  // 否则"自动刷新后专辑封面不见了"这种偏差很难查。
  cleanups.push(scan.watchAutoRefresh());

  // beets 运行时安装进度（docs/29 批 C）：低频事件，但只该订阅一次
  cleanups.push(ui.watchBeetsInstall());

  // 曲库维护事件（docs/29 批 D）：同样是"组件不直接 listen"，只订阅一次
  cleanups.push(ui.watchBeetsMaint());

  /**
   * 启动守卫：引导链挂起（某条 IPC 永不 resolve）时 8s 放行主界面。
   *
   * `booted` 的语义是"首屏数据到过没有"，这里置位是**配合错误条一起**的：
   * TrackListPanel 会因此走「列表加载失败 / 重试」而不是永远"正在启动…"，
   * 也不会误显示"曲库还是空的"（那条只在无错误时出现）。
   */
  const guardTimer = setTimeout(() => {
    if (list.booted) return;
    list.booted = true;
    notify.showError('部分数据未加载：启动引导超时，可重试或重启应用');
  }, BOOT_TIMEOUT_MS);

  void (async () => {
    // 三件事并发：应用信息（dataDir）、播放器快照、界面状态/设置/歌单
    const [dataDir, playerResult] = await Promise.all([library.fetchInfo(), player.fetchState()]);
    const restoredScrollTop = await ui.bootstrap();

    if (dataDir) library.dataDir = dataDir;
    ui.scrollTop = restoredScrollTop;
    // 频谱面板若被恢复，`AppShell` 的 effect 会自己拿源（sidePanel 已就位）

    // 歌单摘要必须在 `ui.bootstrap()` 之后拉：恢复出来的 view 可能指着一个
    // 已经不存在的歌单，`playlists.refresh()` 会顺手把视图退回曲库。
    await playlists.refresh();

    await library.loadStats();
    await list.reload({ restore: true });

    // fetchState 与 player:state 事件并发：事件已先到时 store 里已是新状态，
    // 丢弃这份晚到的快照，避免把界面盖回旧值。
    if (playerResult && !stateEventSeen) {
      player.state = playerResult;
      options.onState(playerResult);
      // 装载由主进程补发的 load 指令完成（attach 时一次性发出）；
      // 音量/静音/归一化全部以后端状态为准，UI 不再"对齐"一遍。
      await player.refreshQueue(playerResult.queueVersion);
    }
  })()
    .catch((err) => {
      // 引导失败不致命：设置/歌单走默认值，界面仍可用
      console.warn('boot failed:', errorMessage(err));
    })
    .finally(() => clearTimeout(guardTimer));

  return () => {
    clearTimeout(guardTimer);
    for (const cleanup of cleanups) cleanup();
  };
}
