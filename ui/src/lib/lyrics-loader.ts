import type { ApiError } from './ipc/generated/ApiError';
import type { Lyrics } from './ipc/generated/Lyrics';

/**
 * 歌词加载状态机。
 *
 * 看起来只是"取一份歌词"，实际有三个容易出错的点：
 * 1. 快速切歌时两次请求可能乱序返回，晚到的旧结果会覆盖新歌的歌词 —— 用请求号丢弃
 * 2. 面板打开/换歌/手动点开三个入口都会触发加载，必须幂等，否则同一首会被反复取
 * 3. 没有歌词的曲目会被反复请求 —— 需要"已确认没有"的负缓存，但要设上限，
 *    否则听过几千首之后这个映射本身就是一处泄漏（虽然很小，但没有边界的东西迟早出问题）
 *
 * 2026-10-31 修（发布前审查 P1-7）：此前是 `attempted: Set`（只记"试过"），
 * 切回一首加载过的歌会被它拦住——既不重发请求也不重发缓存，面板就在 A 的
 * 标题下渲染 B 的歌词（纯音乐与有词歌交替时必现）。改成 `cache: Map`：命中
 * 且不是当前曲目 → 直接重发缓存；只有"确认没有"（notFound/invalidArg）才进
 * 负缓存，临时性失败（io/设备丢失）允许下次重试。
 */

export interface LyricsState {
  lyrics: Lyrics | null;
  loading: boolean;
  trackId: string | null;
}

const CACHE_LIMIT = 500;

export class LyricsLoader {
  /** trackId → 歌词；值 null = 已确认没有歌词（负缓存） */
  private cache = new Map<string, Lyrics | null>();
  private current: string | null = null;
  private request = 0;
  private state: LyricsState = { lyrics: null, loading: false, trackId: null };

  /**
   * `fetchLyrics` **失败即抛出**（新契约的 `ApiError`），由 load() 统一吞成空态：
   * 没有歌词是常态（纯音乐、未匹配到同名 .lrc），不该弹错误条。
   */
  constructor(
    private readonly fetchLyrics: (trackId: string) => Promise<Lyrics>,
    private readonly emit: (state: LyricsState) => void,
  ) {}

  get snapshot(): LyricsState {
    return this.state;
  }

  /** 幂等：同一首已加载过（或已确认没有歌词）就不再请求 */
  load(trackId: string | null): void {
    if (!trackId) return;
    if (this.current === trackId) return;
    // 切回加载过的歌：直接重发缓存（含负缓存 = 立即空态）
    if (this.cache.has(trackId)) {
      this.current = trackId;
      this.publish({ lyrics: this.cache.get(trackId) ?? null, loading: false, trackId });
      return;
    }

    this.current = trackId;
    const ticket = (this.request += 1);
    this.publish({ lyrics: null, loading: true, trackId });

    void this.fetchLyrics(trackId)
      .then((lyrics) => {
        if (ticket !== this.request) return;
        this.remember(trackId, lyrics);
        this.publish({ lyrics, loading: false, trackId });
      })
      .catch((e) => {
        if (ticket !== this.request) return;
        // 只有"确认没有"才进负缓存；io/设备丢失等临时性失败允许下次重试。
        // 非契约形状的异常（不该发生）同样不缓存。
        const code = (e as ApiError | undefined)?.code;
        if (code === 'notFound' || code === 'invalidArg') {
          this.remember(trackId, null);
        }
        this.publish({ lyrics: null, loading: false, trackId });
      });
  }

  /** 曲库被清空/切库时用；正常切歌不需要调用 */
  reset(): void {
    this.request += 1;
    this.cache.clear();
    this.current = null;
    this.publish({ lyrics: null, loading: false, trackId: null });
  }

  private remember(trackId: string, lyrics: Lyrics | null): void {
    if (this.cache.size >= CACHE_LIMIT) this.cache.clear();
    this.cache.set(trackId, lyrics);
  }

  private publish(state: LyricsState): void {
    this.state = state;
    this.emit(state);
  }
}
