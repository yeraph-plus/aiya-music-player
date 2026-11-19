import type { MockOptions } from '../lib/ipc/mock';
import type { MockSeed } from '../lib/ipc/mock/backend';
import { AMBIENT_SCHEMES, BG_SPECTRUM_STYLES, SPECTRUM_STYLES, normalizeUiScale } from '../lib/settings';
import { stationCommands } from './stations';

const PAGES = ['home', 'library', 'playlist', 'albums', 'artists', 'album', 'artist', 'search', 'nowPlaying', 'settings', 'beets', 'live', 'broadcast'] as const;
const SCENES = ['standard', 'long', 'empty', 'issues', 'runtime-error'] as const;

/** 开发态查询参数仅在装配时形成契约形状的数据，不写 UI store。 */
export function previewOptions(params: URLSearchParams): MockOptions {
  const web = params.get('host') === 'web';
  if (web) (window as { __AIYA_WEB__?: object }).__AIYA_WEB__ = {};
  const scene = SCENES.find(value => value === params.get('scene')) ?? 'standard';
  const page = PAGES.find(value => value === params.get('page'));
  const seeded = !!page || params.has('scene') || params.has('theme') || params.has('lang') || params.has('scale') || params.has('bg');
  const seed: MockSeed = {
    ...(seeded ? {
      settings: {
        theme: params.get('theme') === 'light' ? 'light' : 'dark',
        language: params.get('lang') === 'en' ? 'en' : 'zh',
        uiScale: normalizeUiScale(Number(params.get('scale') ?? 100)),
        npSpectrum: params.get('fg') !== 'off',
        bgSpectrum: params.get('bg') !== 'off',
        spectrumStyle: SPECTRUM_STYLES.find(item => item.value === params.get('fg'))?.value ?? 'orbit',
        bgSpectrumStyle: BG_SPECTRUM_STYLES.find(item => item.value === params.get('bg'))?.value ?? 'ridge',
        ambient: AMBIENT_SCHEMES.find(item => item.value === params.get('ambient'))?.value ?? 'aurora',
      },
      uiState: {
        page: page ?? 'home', playlistId: 'pl-1', albumId: page === 'album' ? 'al-01' : null,
        artistId: page === 'artist' ? 'ar-01' : null, scrollTop: 0, sort: 'title', direction: 'asc',
      },
    } : {}),
    ...(web ? { roots: scene === 'empty' ? [] : ['/music'] } : {}),
    empty: scene === 'empty',
    startTrack: page === 'nowPlaying' || params.get('track') === '1',
    track: (track, index) => ({
      ...track,
      artworkId: index % 9 === 8 ? null : `preview-cover-${Number(track.albumId?.slice(3) ?? 0) % 3}`,
      ...(scene === 'long' ? {
        title: index === 0 ? '一首关于漫长旅程与遥远星空的歌 · A very long track title beyond the quiet horizon' : track.title,
        artist: track.artistId === 'ar-01' ? '周深与远方的朋友们 / A collaboration with a very long artist name' : track.artist,
        album: track.albumId === 'al-01' ? '沿着星光走向没有尽头的夜晚 / A long journey through the midnight sky' : track.album,
      } : {}),
    }),
    playlist: (playlist, index) => ({
      ...playlist, artworkId: `preview-cover-${index % 3}`,
      ...(scene === 'long' && index === 0 ? { name: '在漫长的旅途中循环播放 / A playlist for the road beyond the horizon' } : {}),
    }),
    ...(params.get('lyrics') === 'none' ? {} : { lyrics: {
      found: true, synced: true, offsetMs: 0, encoding: 'utf-8',
      lines: [
        '这是开发预览歌词', '晚风走过安静的街道', '灯光映在窗边', '让音乐留在此刻',
        'A quiet light across the room', 'We follow every note into the night',
        '这一行用来检验很长的歌词在窄窗口与放大字号下如何换行和保持完整',
        '远方的回声慢慢靠近', '星光停在眼前', 'And let the music carry us home',
      ].map((text, index) => ({ timeMs: index * 8000, text })),
    } }),
  };
  return { seed, commands: mock => stationCommands(mock, scene), delayMs: Math.min(3000, Math.max(0, Number(params.get('delay')) || 0)) };
}
