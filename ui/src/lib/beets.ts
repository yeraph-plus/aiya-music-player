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

export interface CandidatePreview {
  id: string;
  /** 0–1，1 - distance；低置信度 UI 不默认勾选 */
  confidence: number;
  fromScratch: boolean;
  extraItems: number;
  extraTracks: number;
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
  /** 单张专辑应用完成；failed 是逐曲失败明细（可重试），code=conflict 表示目标路径已存在 */
  | { type: 'applied'; session: string; albumId: string; written: number; moved: boolean; failed: Array<{ path: string; message: string; code?: string }> }
  | { type: 'applyDone'; session: string; applied: number; skipped: number; failed: number; moved: number }
  /* ---- B3：取消 ---- */
  /** 用户取消；session 收尾到最近的检查点为止（已完成的部分保留） */
  | { type: 'cancelled'; session: string; stage: string }
  /** sidecar 收到取消命令的回执 */
  | { type: 'cancelAck' }
  /* ---- 读模型对齐（beets 写权威 → 自有 DB 读模型，docs/08 §5 自闭环） ---- */
  /** beets 库 → 自有 DB 全量对齐汇总（apply 完成后自动触发，也可手动） */
  | { type: 'librarySynced'; matched: number; updated: number; imported: number; unchanged: number; orphaned: number }
  | { type: 'librarySyncFailed'; message: string }
  /** 正在扫描全库，本次对齐让位（可手动再触发） */
  | { type: 'libraryRefreshSkipped'; reason: string }
  | { type: 'sessionEnd' };
