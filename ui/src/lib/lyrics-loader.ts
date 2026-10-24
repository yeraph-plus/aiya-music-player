import type { Lyrics } from '@aiya/contracts';

/**
 * 歌词加载状态机。
 *
 * 看起来只是"取一份歌词"，实际有三个容易出错的点：
 * 1. 快速切歌时两次请求可能乱序返回，晚到的旧结果会覆盖新歌的歌词 —— 用请求号丢弃
 * 2. 面板打开/换歌/手动点开三个入口都会触发加载，必须幂等，否则同一首会被反复取
 * 3. 没有歌词的曲目会被反复请求 —— 需要一层"已尝试"的负缓存，但要设上限，
 *    否则听过几千首之后这个集合本身就是一处泄漏（虽然很小，但没有边界的东西迟早出问题）
 */

export interface LyricsState {
  lyrics: Lyrics | null;
  loading: boolean;
  trackId: string | null;
}

const ATTEMPT_LIMIT = 500;

export class LyricsLoader {
  private attempted = new Set<string>();
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
    if (this.current === trackId || this.attempted.has(trackId)) return;

    this.current = trackId;
    this.remember(trackId);
    const ticket = (this.request += 1);
    this.publish({ lyrics: null, loading: true, trackId });

    void this.fetchLyrics(trackId)
      .then((lyrics) => {
        if (ticket !== this.request) return;
        this.publish({ lyrics, loading: false, trackId });
      })
      .catch(() => {
        if (ticket !== this.request) return;
        this.publish({ lyrics: null, loading: false, trackId });
      });
  }

  /** 曲库被清空/切库时用；正常切歌不需要调用 */
  reset(): void {
    this.request += 1;
    this.attempted.clear();
    this.current = null;
    this.publish({ lyrics: null, loading: false, trackId: null });
  }

  private remember(trackId: string): void {
    if (this.attempted.size >= ATTEMPT_LIMIT) this.attempted.clear();
    this.attempted.add(trackId);
  }

  private publish(state: LyricsState): void {
    this.state = state;
    this.emit(state);
  }
}
