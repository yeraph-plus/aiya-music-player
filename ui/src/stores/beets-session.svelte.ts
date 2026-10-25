import type { AlbumPreview, BeetsImportEvent } from '../lib/beets';
import { events, ipc, type ApplyDecision } from '../lib/ipc';
import { errorMessage } from '../lib/errors';

/**
 * beets 导入会话 store（模块级单例）。
 *
 * `beets:import` 事件在模块加载时全局 listen 一次，不随组件生命周期：
 * 之前 ImportPanel 自己订阅，面板一关监听就拆，刷新/重开面板会丢掉
 * 进行中的会话；预览 → 应用 → 对齐的流转状态也散在组件里。
 * 现在会话状态（phase / 进度快照 / 错误 / 应用结果）都在这里持久，
 * 组件只消费快照并调用命令（beets_preview / beets_apply / beets_cancel）。
 *
 * 启动恢复：宿主在会话进行中刷新 UI 时没有快照命令，store 从 idle 开始
 * （与旧行为一致）。事件处理对乱序/重复 sessionEnd 幂等：settle() 只会把
 * phase 归 idle，不会清数据，重复到达无害。
 *
 * 事件 type 清单见 contracts/src/domain.ts 的 BeetsImportEvent：
 * album / progress / albumError / applied / applyDone / cancelled /
 * cancelAck / librarySynced / librarySyncFailed / libraryRefreshSkipped /
 * done / sessionEnd（没有独立的 sessionFailed，失败语义由
 * librarySyncFailed 与命令失败时的 failed phase 承担）。
 */
export type BeetsPhase = 'idle' | 'previewing' | 'applying' | 'syncing' | 'failed';

/** 专辑 id → 应用结果（单张专辑的应用明细） */
export type BeetsApplyResult = {
  written: number;
  moved: boolean;
  failed: Array<{ path: string; message: string; code?: string }>;
};

class BeetsSessionStore {
  /** 会话阶段：命令发起时进入，收尾事件（done/applyDone 链/cancelled/sessionEnd）归 idle */
  phase = $state<BeetsPhase>('idle');
  /** 最近一次错误（命令失败或对齐失败），展示用；新一轮预览时清空 */
  lastError = $state<string | null>(null);

  /** 预览出的专辑列表（事件流累积） */
  albums = $state<AlbumPreview[]>([]);
  /** 专辑 id → 选中的候选 id（null = 保持当前标签）；store 只写默认值（strong 才默认选第一个） */
  picked = $state<Record<string, string | null>>({});
  /** 最近一次事件 payload 快照：预览进度 / 应用进度 */
  progress = $state<{ current: number; total: number } | null>(null);
  applyProgress = $state<{ current: number; total: number } | null>(null);
  /** 专辑 id → 应用结果 */
  results = $state<Record<string, BeetsApplyResult>>({});
  applySummary = $state<{ applied: number; skipped: number; failed: number; moved: number } | null>(null);
  /** 最近一次曲库对齐汇总 */
  synced = $state<{ matched: number; updated: number; imported: number; unchanged: number; orphaned: number } | null>(null);
  /** 逐条会话错误（albumError / 对齐失败 / 取消提示） */
  errors = $state<Array<{ path: string; message: string }>>([]);
  /** 预览完成的分组数（null = 本轮还没有 done） */
  doneCount = $state<number | null>(null);

  /** 每次 librarySynced 递增；消费方据此触发主视图重拉（刷新逻辑不进 store） */
  syncVersion = $state(0);

  constructor() {
    // 模块级订阅：面板开关不拆监听（否则刷新/重开面板会丢掉进行中的会话）
    events.beetsImport((evt) => this.onEvent(evt as unknown as BeetsImportEvent));
  }

  /* ---------------- 命令 ----------------
   * 组件只调这四个方法、只读上面的快照，不再自己 invoke（docs/18 §4.1）。
   * 失败统一写 `lastError` + `phase='failed'`，不抛：会话组件没有 catch 的位置，
   * 而错误就该显示在面板里（那里已经有一块"需要 beets 2.14+"的提示区）。
   */

  /**
   * 面板主按钮：弹目录选择器 → 清空上一轮 → 发起预览。
   *
   * 选择器与预览合成一个动作：用户点了"选择文件夹预览"，中途取消就是没发生，
   * 不该留下半个已重置的会话。目录来自 `library_choose_root`（同一个系统对话框，
   * 与曲库根目录无关，后端只负责返回路径）。
   */
  async chooseAndPreview(): Promise<void> {
    this.lastError = null;
    let path: string | null = null;
    try {
      const chosen = await ipc.library.chooseRoot();
      path = chosen?.path ?? null;
    } catch (e) {
      this.lastError = errorMessage(e);
      return;
    }
    if (!path) return; // 用户取消：不动会话状态
    this.reset();
    await this.preview([path]);
  }

  /** 选文件夹 → 只读预览。用户取消选择返回 false（不是错误） */
  async preview(paths: string[]): Promise<boolean> {
    this.lastError = null;
    try {
      await ipc.beets.preview(paths);
    } catch (e) {
      this.lastError = errorMessage(e);
      this.phase = 'failed';
      return false;
    }
    this.phase = 'previewing';
    return true;
  }

  /** 应用决策（写标签 + 可选移动/复制 + 随后自动对齐读模型） */
  async apply(decisions: ApplyDecision[]): Promise<void> {
    this.lastError = null;
    this.results = {};
    this.applySummary = null;
    this.synced = null;
    try {
      await ipc.beets.apply(decisions);
    } catch (e) {
      this.lastError = errorMessage(e);
      this.phase = 'failed';
      return;
    }
    this.phase = 'applying';
  }

  /** 取消当前预览/应用：sidecar 收尾到最近检查点，已完成部分保留 */
  async cancel(): Promise<void> {
    try {
      await ipc.beets.cancel();
    } catch (e) {
      // 取消失败不阻塞流程：sidecar 会在 sessionEnd 自己收尾
      console.warn('beets.cancel failed:', errorMessage(e));
    }
  }

  /** 手动全量对齐：beets 库 → 自有读模型（无需先预览） */
  async syncNow(): Promise<void> {
    this.lastError = null;
    this.synced = null;
    try {
      await ipc.beets.sync();
    } catch (e) {
      this.lastError = errorMessage(e);
      this.phase = 'failed';
      return;
    }
    this.phase = 'syncing';
  }

  /** 清空会话数据回到 idle（新一轮预览前调用） */
  reset(): void {
    this.phase = 'idle';
    this.lastError = null;
    this.albums = [];
    this.picked = {};
    this.progress = null;
    this.applyProgress = null;
    this.results = {};
    this.applySummary = null;
    this.synced = null;
    this.errors = [];
    this.doneCount = null;
  }

  private onEvent(evt: BeetsImportEvent): void {
    switch (evt.type) {
      case 'album': {
        this.albums = [...this.albums, evt.album];
        // 默认选择：只有 strong 才默认选第一个候选（docs/08 §4.1 低置信度不默认勾选）
        const strong = evt.album.candidates[0];
        this.picked = {
          ...this.picked,
          [evt.album.id]: evt.album.recommendation === 'strong' && strong ? strong.id : null,
        };
        break;
      }
      case 'progress':
        if (evt.stage === 'writing') {
          this.applyProgress = { current: evt.current, total: evt.total };
          if (this.phase === 'idle') this.phase = 'applying';
        } else {
          this.progress = { current: evt.current, total: evt.total };
          if (this.phase === 'idle') this.phase = 'previewing';
        }
        break;
      case 'albumError':
        this.errors = [...this.errors, { path: evt.path, message: evt.beets_message }];
        break;
      case 'applied':
        this.results = {
          ...this.results,
          [evt.albumId]: { written: evt.written, moved: evt.moved, failed: evt.failed },
        };
        break;
      case 'applyDone':
        this.applySummary = { applied: evt.applied, skipped: evt.skipped, failed: evt.failed, moved: evt.moved };
        // 应用完成后自动触发读模型对齐，等 librarySynced 系列事件收尾
        if (this.phase === 'applying') this.phase = 'syncing';
        break;
      case 'cancelled':
        // 取消收尾：已完成的部分保留，归 idle 解锁按钮
        this.errors = [...this.errors, { path: 'cancel', message: '已取消（已完成的部分保留）' }];
        this.settle();
        break;
      case 'cancelAck':
        // sidecar 回执，无需状态变化
        break;
      case 'librarySynced':
        this.synced = {
          matched: evt.matched,
          updated: evt.updated,
          imported: evt.imported,
          unchanged: evt.unchanged,
          orphaned: evt.orphaned,
        };
        this.syncVersion += 1;
        this.settle();
        break;
      case 'librarySyncFailed':
        this.errors = [...this.errors, { path: 'sync', message: evt.message }];
        this.lastError = evt.message;
        this.settle();
        break;
      case 'libraryRefreshSkipped':
        this.errors = [...this.errors, { path: 'sync', message: '有扫描正在进行，本次对齐让位；稍后再试' }];
        this.settle();
        break;
      case 'done':
        this.doneCount = evt.albums;
        this.settle();
        break;
      case 'sessionEnd':
        this.settle();
        break;
    }
  }

  /** 会话收尾：幂等归 idle（容忍乱序/重复 sessionEnd），不清数据 */
  private settle(): void {
    if (this.phase !== 'idle') this.phase = 'idle';
  }
}

export const beetsSession = new BeetsSessionStore();
