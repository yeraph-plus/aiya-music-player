import type { MockBackend } from '../lib/ipc/mock/backend';
import type { MockCommands } from '../lib/ipc/mock';
import type { ApiError, StationDetail, StationProgram, StationProgramEdit, StationQueueItem, StationSummary } from '../lib/ipc/generated';
import { LIBRARY_SOURCE, queueItemError } from '../lib/station-time';

const clone = <T,>(value: T): T => structuredClone(value);

function failure(code: ApiError['code'], message: string): never {
  throw { code, message, detail: 'mock station', retryable: false } satisfies ApiError;
}

/** 只提供频道编辑与运行状态的内存回执，不模拟节目调度或媒体出流。 */
export function stationCommands(mock: MockBackend, scene: string): MockCommands {
  const stations = new Map<string, StationDetail>();
  let nextId = 3;
  const sourceItems: StationQueueItem[] = [
    { playlistId: LIBRARY_SOURCE, playMode: 'shuffle', priority: 0, scheduled: false, slots: [] },
    { playlistId: 'pl-1', playMode: 'ordered', priority: 1, scheduled: true, slots: [
      { daysMask: 31, startHm: '09:00', endHm: '12:00' },
      { daysMask: 96, startHm: '18:00', endHm: '24:00' },
    ] },
  ];
  const blankProgram = (): StationProgram => ({
    revision: '1', items: [], sources: [], needsApply: false,
    timezone: 'Asia/Shanghai', issues: [], error: null,
  });
  if (scene !== 'empty') {
    for (const [id, name, active] of [
      ['preview-night', scene === 'long' ? '深夜自习室 · 在城市尽头听一首很长很长的歌 / A very long channel title for quiet evenings' : '深夜自习室', true],
      ['preview-day', 'Daylight Radio', false],
    ] as const) {
      stations.set(id, {
        id, name, subtitle: 'Local library · Preview', description: '系统歌单与全曲库的节目配置预览 / Playlist scheduling preview.',
        coverUrl: '/src/dev/assets/preview-cover-1.svg', hlsUrl: `/hls/${id}/live.m3u8`,
        visibility: active ? 'public' : 'private', active, running: active,
        nowPlaying: active ? '预览曲目 / Preview track' : null,
        program: { ...blankProgram(), items: clone(sourceItems) },
      });
    }
    if (scene === 'issues') {
      const first = stations.get('preview-night')!;
      first.program.items.push({ playlistId: 'deleted-playlist', playMode: 'ordered', priority: 2, scheduled: false, slots: [] });
      first.program.needsApply = true;
    }
    if (scene === 'runtime-error') {
      const first = stations.get('preview-night')!;
      first.running = false;
      first.program.error = 'Preview: Liquidsoap process unavailable';
    }
  }

  function get(id: unknown): StationDetail {
    return stations.get(String(id)) ?? failure('notFound', '预览频道不存在 / Channel not found');
  }
  function program(station: StationDetail): StationProgram {
    const playlists = mock.playlistList();
    const total = mock.libraryStats().tracks;
    const ids = [...new Set([LIBRARY_SOURCE, ...station.program.items.map(item => item.playlistId)])];
    const sources = ids.map(playlistId => {
      const playlist = playlists.find(item => item.id === playlistId);
      const library = playlistId === LIBRARY_SOURCE;
      const count = library ? total : playlist?.trackCount ?? 0;
      return { playlistId, name: library ? '' : playlist?.name ?? playlistId, trackCount: count, availableCount: count, exists: library || !!playlist };
    });
    const issues: StationProgram['issues'] = [];
    if (!station.program.items.length) issues.push({ kind: 'emptyQueue' });
    for (const item of station.program.items) {
      const source = sources.find(source => source.playlistId === item.playlistId)!;
      if (!source.exists) issues.push({ kind: 'missingSource', playlistId: item.playlistId });
      else if (!source.availableCount) issues.push({ kind: 'emptySource', playlistId: item.playlistId });
    }
    return { ...station.program, sources, issues };
  }
  function summary(station: StationDetail): StationSummary {
    const { program: _program, ...summary } = station;
    return clone(summary);
  }
  function info(args: Record<string, unknown>) {
    const name = String(args.name ?? '').trim();
    if (!name) failure('invalidArg', '频道名不能为空 / Channel name is required');
    const optional = (key: string) => typeof args[key] === 'string' ? String(args[key]).trim() || null : null;
    return { name, subtitle: optional('subtitle'), description: optional('description'), coverUrl: optional('coverUrl'), visibility: args.visibility === 'public' ? 'public' : 'private' };
  }
  return {
    station_list: () => [...stations.values()].map(summary),
    station_get: args => {
      const station = get(args.stationId);
      return clone({ ...station, program: program(station) });
    },
    station_create: args => {
      const id = `preview-${nextId++}`;
      const station: StationDetail = { ...info(args), id, hlsUrl: `/hls/${id}/live.m3u8`, active: false, running: false, nowPlaying: null, program: blankProgram() };
      stations.set(id, station);
      return summary(station);
    },
    station_update: args => {
      const station = get(args.stationId);
      Object.assign(station, info(args));
      return summary(station);
    },
    station_delete: args => { stations.delete(get(args.stationId).id); },
    station_set_program: args => {
      const station = get(args.stationId);
      const edit = args as unknown as StationProgramEdit;
      if (edit.revision !== station.program.revision) failure('busy', '预览配置已更新，请重新打开 / Reopen the updated configuration');
      if (!Array.isArray(edit.items) || edit.items.some(item => queueItemError(item, edit.items))) failure('invalidArg', '节目配置无效 / Invalid program');
      station.program = { ...station.program, revision: String(Number(edit.revision) + 1), items: clone(edit.items), needsApply: true, error: null };
      return clone(program(station));
    },
    station_apply: args => {
      const station = get(args.stationId);
      if (program(station).issues.length) failure('invalidArg', '节目来源不可用 / Program source unavailable');
      station.program.needsApply = false;
      station.program.error = null;
      return clone(program(station));
    },
    station_activate: args => {
      const station = get(args.stationId);
      if (args.active && program(station).issues.length) failure('invalidArg', '节目来源不可用 / Program source unavailable');
      station.active = args.active === true;
      station.running = station.active;
      station.program.needsApply = false;
      station.program.error = null;
      return summary(station);
    },
    station_skip: args => {
      const station = get(args.stationId);
      if (!station.running) failure('busy', '频道未开播 / Channel is offline');
      station.nowPlaying = '下一首预览曲目 / Next preview track';
      return { raw: 'preview: skipped' };
    },
    station_push: args => { get(args.stationId); return { raw: 'preview: queued' }; },
  };
}
