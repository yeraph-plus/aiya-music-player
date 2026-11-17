const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('../ui/node_modules/typescript');

test('broadcast translations preserve parameters, source names and both languages', () => {
  const cache = new Map();
  function load(file) {
    if (cache.has(file)) return cache.get(file);
    const exports = {};
    cache.set(file, exports);
    const ctx = { exports, $state: value => value, require: name => {
      const base = path.resolve(path.dirname(file), name);
      return load(fs.existsSync(`${base}.ts`) ? `${base}.ts` : `${base}.svelte.ts`);
    } };
    vm.runInNewContext(ts.transpileModule(fs.readFileSync(file, 'utf8'), {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText, ctx);
    return exports;
  }
  const root = path.resolve(__dirname, '../ui/src/lib');
  const { stationZh, stationEn } = load(path.join(root, 'i18n/domains/station.ts'));
  assert.deepEqual(Object.keys(stationZh).sort(), Object.keys(stationEn).sort());
  const parameters = value => Array.from(value.matchAll(/\{(\w+)\}/g), match => match[1]).sort();
  for (const key of Object.keys(stationZh)) {
    assert.deepEqual(parameters(stationZh[key]), parameters(stationEn[key]), key);
    assert.ok(stationEn[key].trim(), key);
  }
  const { t, setLocale } = load(path.join(root, 'i18n.svelte.ts'));
  const { programIssueText } = load(path.join(root, 'station-i18n.ts'));
  const name = 'My $& {visibility} playlist';
  const sources = [{ playlistId: 'p', name }];
  for (const locale of ['zh', 'en']) {
    setLocale(locale);
    const dictionary = locale === 'zh' ? stationZh : stationEn;
    assert.equal(programIssueText({ kind: 'emptyQueue' }, sources), dictionary['station.issueEmptyQueue']);
    assert.equal(programIssueText({ kind: 'missingSource', playlistId: 'p' }, sources),
      dictionary['station.issueMissingSource'].replace('{name}', () => name));
    assert.equal(programIssueText({ kind: 'missingSource', playlistId: 'deleted' }, sources),
      dictionary['station.issueMissingSource'].replace('{name}', 'deleted'));
    assert.equal(programIssueText({ kind: 'emptySource', playlistId: '__library__' }, sources),
      dictionary['station.issueEmptySource'].replace('{name}', dictionary['station.allLibrary']));
    assert.equal(programIssueText({ kind: 'uncoveredTime', weekday: 0, time: '08:30' }, sources),
      locale === 'zh' ? '周一 08:30 未安排节目，请添加常规轮播歌单或补全时段。'
        : 'No program at Mon 08:30. Add a regular playlist or complete the schedule.');
    assert.equal(t('station.configFor', { name, visibility: 'rewritten' }),
      dictionary['station.configFor'].replace('{name}', () => name));
  }
});

test('weekly picker keeps partial hours, full week, Sunday wrap and shared priority', () => {
  const ctx={exports:{}};
  const source=fs.readFileSync(path.join(__dirname,'../ui/src/lib/station-time.ts'),'utf8');
  vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText,ctx);
  const {covers,conflicts,validSlot,programError}=ctx.exports;
  const night={startHm:'22:30',endHm:'02:15',daysMask:64};
  assert.equal(covers(night,6,1350),true);assert.equal(covers(night,0,134),true);assert.equal(covers(night,0,135),false);
  assert.equal(covers(night,1,30),false);
  const other={startHm:'01:00',endHm:'02:00',daysMask:1};
  const items=[{playlistId:'p',priority:0,scheduled:true,slots:[night]},{playlistId:'q',priority:0,scheduled:true,slots:[other]}];
  assert.equal(conflicts(items,other,'q'),true);
  assert.equal(programError(items),'station.timeConflict');
  items[1].priority=1;
  assert.equal(conflicts(items,other,'q'),false);
  assert.equal(programError(items),null);
  const full={startHm:'00:00',endHm:'24:00',daysMask:127};
  for(let day=0;day<7;day++)assert.equal(covers(full,day,0,1440),true);
  assert.equal(validSlot({...full,startHm:'24:00'}),false);
  assert.equal(validSlot({...full,endHm:'24:01'}),false);
  assert.equal(validSlot({...full,daysMask:0}),false);
  assert.equal(programError([{...items[0],slots:[full,night]}]),null);
});

test('station refresh removes deleted service cards and preserves manual streams', () => {
  const ctx = { exports: {}, require: (name) => name === './format' ? { STREAM_ID_PREFIX: 'stream:' } : { t: (key) => key } };
  const source = fs.readFileSync(path.join(__dirname, '../ui/src/lib/live.ts'), 'utf8');
  vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText, ctx);
  const service = { id: 'keep', url: '/hls/keep/live.m3u8', title: 'Old', source: 'service', addedAtMs: 1 };
  const manual = { id: 'manual', url: '/hls/match/live.m3u8', title: 'My title', source: 'manual', addedAtMs: 2 };
  const summary = (id, name) => ({ id, name, hlsUrl: `/hls/${id}/live.m3u8`, subtitle: null, description: null, coverUrl: null });
  const result = ctx.exports.mergeServiceStations([service, { ...service, id: 'deleted' }, manual], [summary('keep', 'Updated'), summary('match', 'Server title')]);
  assert.deepEqual(Array.from(result, (entry) => entry.id), ['keep', 'manual']);
  assert.equal(result[0].title, 'Updated');
  assert.equal(result[1].title, 'My title');
  assert.equal(result[1].source, 'manual');
  assert.deepEqual(Array.from(ctx.exports.mergeServiceStations(result, []), (entry) => entry.id), ['manual']);
});

function harness() {
  let transport;
  let fetcher;
  const audios = [], sources = [], hlsInstances = [];
  class Audio {
    constructor() {
      this.style = {}; this.currentTime = 0; this.duration = 10; this.playbackRate = 1;
      this.paused = true; this.handlers = {}; audios.push(this);
    }
    addEventListener(name, cb) { this.handlers[name] = cb; }
    canPlayType() { return ''; }
    play() { this.paused = false; return Promise.resolve(); }
    pause() { this.paused = true; }
    removeAttribute(name) { delete this[name]; }
    load() {}
  }
  class Hls {
    static Events = { ERROR: 'error' };
    static isSupported() { return true; }
    constructor() { hlsInstances.push(this); }
    on() {}
    loadSource(src) { this.src = src; }
    attachMedia(media) { this.media = media; }
    destroy() { this.destroyed = true; }
  }
  class EventSource {
    constructor() { this.handlers = new Map(); sources.push(this); }
    addEventListener(name, cb) {
      if (!this.handlers.has(name)) this.handlers.set(name, new Set());
      this.handlers.get(name).add(cb);
    }
    removeEventListener(name, cb) { this.handlers.get(name)?.delete(cb); }
    fire(name) { for (const cb of this.handlers.get(name) ?? []) cb({ data: '{}' }); }
    close() { this.closed = true; }
  }
  const ctx = {
    exports: {}, require: (name) => name === 'hls.js' ? Hls : {
      setTransport: (t) => { transport = t; }, transport: () => transport,
    },
    Audio, Hls, EventSource, URLSearchParams, AbortSignal, console,
    window: { location: { search: '?token=secret' } },
    localStorage: { getItem: () => null }, document: { body: { appendChild() {} } },
    setInterval: () => 1, clearInterval() {}, fetch: (...args) => fetcher(...args),
  };
  const source = fs.readFileSync(path.join(__dirname, '../ui/src/lib/ipc/http.ts'), 'utf8');
  const code = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  vm.runInNewContext(code, ctx);
  ctx.exports.installHttpTransport();
  return { transport, audios, sources, hlsInstances, fetch: (fn) => { fetcher = fn; } };
}

const state = (token, id, status = 'playing', source = 'file') => ({
  token, current: { id }, status, source, positionMs: 0, volume: 0.5, muted: false, rate: 1,
});
const response = (body) => ({ ok: true, json: async () => body });

test('playback requests wait for earlier responses before sending later commands', async () => {
  const h = harness();
  let release;
  const requests = [];
  h.fetch(async (url) => {
    requests.push(url);
    return requests.length === 1 ? new Promise((r) => { release = () => r(response(state(1, 'a'))); }) : response(state(2, 'b'));
  });
  const old = h.transport.invoke('player_set_volume', { volume: 0.5 });
  await Promise.resolve();
  const next = h.transport.invoke('player_next');
  await Promise.resolve();
  assert.equal(requests.length, 1);
  release();
  await Promise.all([old, next]);
  assert.match(h.audios[0].src, /\/audio\/b\?/);
});

test('same-track delayed position report cannot resume a later pause', async () => {
  const h = harness();
  let release;
  h.fetch(async (url) => url.includes('player_report')
    ? new Promise((r) => { release = () => r(response(state(2, 'b'))); })
    : response(state(2, 'b', 'paused')));
  const report = h.transport.invoke('player_report', { kind: 'position', token: 2 });
  await Promise.resolve();
  const pause = h.transport.invoke('player_toggle');
  release();
  await Promise.all([report, pause]);
  assert.equal(h.audios[0].paused, true);
});

test('a failed playback request does not block subsequent controls', async () => {
  const h = harness();
  h.fetch(async (url) => {
    if (url.includes('player_next')) throw new Error('offline');
    return response(state(2, 'b', 'paused'));
  });
  await assert.rejects(h.transport.invoke('player_next'));
  await h.transport.invoke('player_toggle');
  assert.equal(h.audios[0].paused, true);
});

test('SSE resubscription dispatches once and recreates a closed source', async () => {
  const h = harness();
  const keep = await h.transport.listen('scan:progress', () => {});
  let count = 0;
  let off = await h.transport.listen('beets:install', () => count++);
  off();
  off = await h.transport.listen('beets:install', () => count++);
  h.sources[0].fire('beets:install');
  assert.equal(count, 1);
  off(); keep();
  assert.equal(h.sources[0].closed, true);
  await h.transport.listen('beets:install', () => count++);
  h.sources[1].fire('beets:install');
  assert.equal(count, 2);
});

test('HLS streams use authenticated media URLs and release the previous engine', async () => {
  const h = harness();
  let result = state(1, 'stream:/hls/test/live.m3u8', 'loading', 'stream');
  h.fetch(async () => response(result));
  await h.transport.invoke('live_open');
  assert.equal(h.hlsInstances[0].src, '/hls/test/live.m3u8?token=secret');
  result = state(2, 'stream:https://example.com/live.m3u8', 'loading', 'stream');
  await h.transport.invoke('live_open');
  assert.equal(h.hlsInstances[0].destroyed, true);
  const src = new URL(h.hlsInstances[1].src, 'http://localhost');
  assert.equal(src.pathname, '/stream');
  assert.equal(src.searchParams.get('u'), 'https://example.com/live.m3u8');
  assert.equal(src.searchParams.get('token'), 'secret');
  result = state(2, result.current.id, 'paused', 'stream');
  await h.transport.invoke('player_toggle');
  assert.equal(h.hlsInstances[1].destroyed, true);
  assert.equal(h.audios[0].src, undefined);
});
