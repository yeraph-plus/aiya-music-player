/**
 * beets 导入协议：预览与事件的前端形状（事件协议见 docs/08 §3.2）。
 *
 * 这些结构没有生成类型：载荷由 Python sidecar 产出，Rust 宿主逐行转发 JSON、不做
 * 强类型转换。本文件是协议的手写镜像，`beets-session` 的 switch 分支按它取字段；
 * sidecar 改字段时两侧同步修改。
 *
 * `ApplyDecision` 不在此文件：它是命令入参，定义在 `lib/ipc/client.ts`。
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
  /** v2：证据层级，说明推荐依据；机器判定读 `evidence`，`reasons` 为展示用短句 */
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
  /* ---- 曲库刷新终态（宿主扫描段；beets 对账不在本事件流，见 docs/43 §8-E） ---- */
  /** 正在扫描全库（写库互斥被占），本次刷新让位（自动触发时不发此事件） */
  | { type: 'libraryRefreshSkipped'; reason: string }
  /* ---- 自动刷新管线终态（docs/33 §7） ---- */
  /**
   * 一次刷新完成（扫描 → 对齐 → 单 bump）。
   *
   * 发出条件：确有写入变化；手动触发例外（用户操作需要可见反馈）。未收到该事件
   * 仅表示无变化，最近一次结果见 `refresh_state` 返回的快照。
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
