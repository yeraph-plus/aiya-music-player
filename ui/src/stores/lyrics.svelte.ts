import type { Lyrics } from '../lib/ipc';
import { ipc } from '../lib/ipc';
import { LyricsLoader } from '../lib/lyrics-loader';

/**
 * 歌词状态：当前曲目的歌词、加载中标记、并发/幂等控制。
 *
 * 为什么从 `ui` 拆出来（2026-10-26）：`ui` 里有 view/search/settings/
 * diagnostics/beets/scrollTop/歌词 七摊事，歌词是唯一「按曲目变化」的那摊
 * —— 换歌就整份替换，和界面状态没有任何交集。拆开后 `ui` 只描述"界面在哪、
 * 长什么样"，歌词的取数逻辑在 loader 里，这一层只是它的状态容器。
 *
 * 并发与幂等由 `LyricsLoader` 负责：面板打开、换歌、点按钮三个入口都会调
 * `load()`，晚到的旧请求会被丢弃（详见 lib/lyrics-loader.ts）。
 */
class LyricsStore {
  /** 当前曲目的歌词文档；没取到或还没取是 null */
  doc = $state<Lyrics | null>(null);
  loading = $state(false);

  private readonly loader = new LyricsLoader(
    (trackId) => ipc.lyrics.get(trackId),
    (state) => {
      this.doc = state.lyrics;
      this.loading = state.loading;
    },
  );

  load(trackId: string): void {
    this.loader.load(trackId);
  }
}

export const lyrics = new LyricsStore();
