/**
 * beets 导入的事件与预览形状（docs/08 §3.2 事件协议）。
 *
 * 为什么没有生成类型：这些结构由 Python sidecar 产出，Rust 宿主只做
 * "逐行 JSON 转发"，不进强类型 DTO（`beets:import` 事件的载荷是动态的）。
 * 所以这里是**前端对协议的手写镜像** —— 改 sidecar 的字段要同步改这里，
 * 且只有 `beets-session` 的 switch 分支会碰到字段（写错一处就编译不过）。
 *
 * 2026-10-25 批次 4：从已删除的 `@aiya/contracts` 搬来。
 * 应用决策（`ApplyDecision`）不在这里：它是**命令入参**，定义在
 * `lib/ipc/client.ts`（`candidate` 原样回传，宿主不解读）。
 */

/** 单曲级标签（当前与候选同形，UI 才能逐字段对齐） */
export interface TrackMeta {
  title: string | number | null;
  artist: string | null;
  album: string | null;
  albumartist: string | null;
  year: number | null;
  track: number | null;
  disc: number | null;
  genre: string | null;
  path?: string;
  length?: number;
}

/**
 * 候选证据层级（协议 v2，借鉴 beets-web-manager ADR 0002/0007）。
 * recordingId > metadataSlot > releaseGroup > similarity，自强到弱。
 * 指纹层（AcoustID）预留未实现——工具侧不虚标，这里也就没有这个值。
 */
export type EvidenceTier = 'recordingId' | 'metadataSlot' | 'releaseGroup' | 'similarity';

export interface CandidatePreview {
  id: string;
  /** 0–1，1 - distance；低置信度 UI 不默认勾选 */
  confidence: number;
  fromScratch: boolean;
  extraItems: number;
  extraTracks: number;
  /** v2：证据层级——回答"为什么推荐这个"（机器判定看它，reasons 是人话） */
  evidence?: EvidenceTier;
  /** v2：判到该层的依据短句 */
  reasons?: string[];
  album: {
    id: string | null;
    album: string | null;
    artist: string | null;
    year: number | null;
    genre: string | null;
    dataSource: string | null;
    tracks: TrackMeta[];
  };
  /** item.path → 候选 TrackInfo（逐曲对齐） */
  mapping: Record<string, TrackMeta>;
}

export interface AlbumPreview {
  id: string;
  paths: string[];
  /** strong / medium / low / none；低置信度不默认勾选 */
  recommendation: 'strong' | 'medium' | 'low' | 'none';
  current: { album: string | null; artist: string | null; tracks: TrackMeta[] };
  candidates: CandidatePreview[];
}

/** `beets:import` 事件流（sidecar 逐行转发） */
export type BeetsImportEvent =
  | { type: 'progress'; session: string; stage: string; current: number; total: number; path: string | null }
  | { type: 'album'; session: string; album: AlbumPreview }
  | { type: 'albumError'; session: string; stage: string; path: string; beets_message: string }
  | { type: 'done'; session: string; albums: number }
  /* ---- B2：应用阶段 ---- */
  /** 单张专辑应用完成；failed 是逐曲失败明细（可重试），code=conflict 表示目标路径已存在，code=blankIdentity 表示合并后身份标签仍空白被拒绝写入 */
  | { type: 'applied'; session: string; albumId: string; written: number; moved: boolean; failed: Array<{ path: string; message: string; code?: string }> }
  /** resumedSkipped（v2）：resume 续跑时因断点已完成而跳过的专辑数 */
  | { type: 'applyDone'; session: string; applied: number; skipped: number; failed: number; moved: number; resumedSkipped?: number }
  /* ---- B3：取消 ---- */
  /** 用户取消；session 收尾到最近的检查点为止（已完成的部分保留） */
  | { type: 'cancelled'; session: string; stage: string }
  /** sidecar 收到取消命令的回执 */
  | { type: 'cancelAck' }
  /* ---- 曲库刷新终态（宿主扫描段；beets 对账已随 docs/43 §8-E 移除） ---- */
  /** 正在扫描全库（写库互斥被占），本次刷新让位（自动触发时不会发这条） */
  | { type: 'libraryRefreshSkipped'; reason: string }
  /* ---- 自动刷新管线终态（docs/33 §7） ---- */
  /**
   * 一次刷新跑完（扫描 → 对齐 → 单 bump）。
   *
   * **只有真的写出变化才发**，唯一例外是手动触发（用户点了按钮必须看到回应）。
   * 所以"没收到这条"不等于管线没在工作 —— 它可能只是没发现变化，
   * 详情看 `refresh_state` 命令返回的最近一次快照。
   */
  | {
      type: 'libraryRefreshed';
      trigger: 'timer' | 'focus' | 'action' | 'manual';
      atMs: number;
      changed: boolean;
      scanned: number;
      imported: number;
      updated: number;
      missing: number;
      failed: number;
      sync: 'ok' | 'skipped' | 'failed';
      elapsedMs: number;
      error: string | null;
    }
  /** 整条管线失败（DB 打不开之类）；beets 段单独失败只在快照里，不走这条 */
  | { type: 'libraryRefreshFailed'; trigger: string; message: string }
  | { type: 'sessionEnd' };
