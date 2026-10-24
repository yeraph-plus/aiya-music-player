import type { PlayerState } from '@aiya/contracts';
import { ARTWORK_SIZES, artworkUrl } from './artwork';
import { artworkObjectUrl } from './artwork-blob.js';

/**
 * Media Session：系统媒体中心 / 锁屏 / 蓝牙耳机的显示与控制。
 *
 * 它和主进程的全局媒体键不是一回事，也不是重复实现：
 * - 媒体键负责"按键 → 播放器"，全局生效，主进程注册
 * - Media Session 负责"播放器 → 系统 UI"，显示歌名封面、进度，
 *   并让蓝牙耳机、锁屏这类系统控件能发指令
 *
 * 两者同时注册不会冲突：Chromium 只在窗口有焦点时把媒体键交给页面，
 * 失焦时走主进程的 globalShortcut。
 *
 * 封面必须是 blob:/data: 之类的"标准" scheme —— `media://` 会被 Blink 拒绝，
 * 所以这里走 artwork-blob 转一次。标题等信息先同步落到系统 UI，
 * 封面就绪后再升级一次 metadata，不让取图的等待拖慢歌名显示。
 *
 * 这一层失败不能影响播放，所有调用都吞异常。
 */

export interface MediaSessionHandlers {
  toggle(): void;
  next(): void;
  prev(): void;
  seek(ms: number): void;
  /** 相对跳转（秒），负数后退 */
  skip(seconds: number): void;
  stop(): void;
}

export interface MediaSessionBridge {
  update(state: PlayerState | null): void;
  dispose(): void;
}

const NOOP: MediaSessionBridge = { update: () => undefined, dispose: () => undefined };

export interface MediaSessionOptions {
  /**
   * 是否允许系统控制（播放/暂停、上一首、下一首、进度）。
   *
   * 关掉时仍然发布标题与封面——系统媒体面板照常显示，但按键不生效。
   * 这对应设置里的"系统媒体键"：不想让本应用接管系统媒体控制的人，
   * 依然能看到当前在放什么。
   */
  actions?: boolean;
}

export function createMediaSession(
  handlers: MediaSessionHandlers,
  options: MediaSessionOptions = {},
): MediaSessionBridge {
  const session = typeof navigator !== 'undefined' ? navigator.mediaSession : undefined;
  if (!session) return NOOP;
  const withActions = options.actions !== false;

  /** 每次 update 自增；异步封面回来时用它丢弃过期结果（快速切歌时的竞态） */
  let generation = 0;
  /**
   * 上一次写进系统 UI 的曲目指纹。
   *
   * 播放位置每 500ms 上报一次，而 update() 跟着每次状态广播跑；
   * 每次都 new MediaMetadata 会让 Windows SMTC / 锁屏反复重设元数据，
   * 有些系统上表现为缩略图闪烁、甚至每条状态都推一次通知。只有曲目变了才重建。
   */
  let metadataKey: string | null = null;
  /** 已经为哪一首取过封面；每首只取一次，取不到也不再重试 */
  let artworkKey: string | null = null;

  const handlersMap: Array<[MediaSessionAction, MediaSessionActionHandler]> = [
    ['play', () => handlers.toggle()],
    ['pause', () => handlers.toggle()],
    ['previoustrack', () => handlers.prev()],
    ['nexttrack', () => handlers.next()],
    ['stop', () => handlers.stop()],
    [
      'seekto',
      (details) => {
        if (typeof details.seekTime === 'number') handlers.seek(details.seekTime * 1000);
      },
    ],
    [
      'seekbackward',
      (details) => handlers.skip(-(details.seekOffset ?? 5)),
    ],
    [
      'seekforward',
      (details) => handlers.skip(details.seekOffset ?? 5),
    ],
  ];

  for (const [action, handler] of handlersMap) {
    if (!withActions) break;
    try {
      session.setActionHandler(action, handler);
    } catch {
      /* 部分动作在旧版 Chromium 不支持 */
    }
  }

  return {
    update(state) {
      const ticket = (generation += 1);
      try {
        if (!state?.current) {
          if (metadataKey !== null) {
            metadataKey = null;
            artworkKey = null;
            session.metadata = null;
          }
          session.playbackState = 'none';
          return;
        }

        const track = state.current;
        const base = {
          title: track.title,
          artist: track.artist ?? '',
          album: track.album ?? '',
        };

        // 先上无缝的信息，封面随后补
        const key = [track.id, base.title, base.artist, base.album, track.artworkId ?? ''].join('\u0001');
        if (key !== metadataKey) {
          metadataKey = key;
          session.metadata = new MediaMetadata(base);
        }
        session.playbackState = state.status === 'playing' ? 'playing' : 'paused';

        const duration = state.durationMs ?? 0;
        if (duration > 0) {
          const position = Math.min(Math.max(state.positionMs, 0), duration) / 1000;
          session.setPositionState?.({
            duration: duration / 1000,
            position,
            // 不传真实速率的话，系统会用 1× 推算进度，2× 播放时进度条越走越落后
            playbackRate: state.rate ?? 1,
          });
        }

        if (key !== artworkKey) {
          // 每首曲目只取一次封面：取不到（无封面/读失败）就保持无封面，不再每帧重试
          artworkKey = key;
          const largest = ARTWORK_SIZES[ARTWORK_SIZES.length - 1] ?? 512;
          const source = artworkUrl(track.artworkId, largest);
          if (source) {
            void artworkObjectUrl(source).then((blobUrl) => {
              // 切歌/关闭后到达的结果直接丢弃，否则系统 UI 会显示上一首的封面
              if (!blobUrl || ticket !== generation) return;
              try {
                session.metadata = new MediaMetadata({
                  ...base,
                  artwork: [{ src: blobUrl, sizes: `${largest}x${largest}`, type: 'image/jpeg' }],
                });
              } catch {
                /* 封面写入失败就保持无封面 */
              }
            });
          }
        }
      } catch {
        /* 系统媒体中心异常不影响播放 */
      }
    },
    dispose() {
      try {
        for (const [action] of handlersMap) session.setActionHandler(action, null);
        session.metadata = null;
      } catch {
        /* 忽略 */
      }
    },
  };
}
