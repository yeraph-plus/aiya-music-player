<script lang="ts">
  import { onMount } from 'svelte';
  import type {
    AppSettings,
    PlaylistSummary,
    Track,
    TrackQuery,
    TrackSort,
  } from '@aiya/contracts';
  import {
    AUDIO_RATE_STEP,
    parseTrackQuery,
    resolveShortcutBindings,
    type ShortcutAction,
  } from '@aiya/contracts';
  import AppShell from './components/AppShell.svelte';
  import TopBar from './components/TopBar.svelte';
  import TrackList from './components/TrackList.svelte';
  import ViewHeader from './components/ViewHeader.svelte';
  import SettingsPanel from './components/SettingsPanel.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import PromptDialog from './components/PromptDialog.svelte';
  import ConfirmDialog from './components/ConfirmDialog.svelte';
  import PlaylistPicker from './components/PlaylistPicker.svelte';
  import { artworkUrl } from './lib/artwork';
  import { errorMessage } from './lib/errors';
  import { TrackWindow } from './lib/track-window';
  import { accentForArtwork } from './lib/accent';
  import { listKeyIntent } from './lib/list-keys';
  import { createMediaSession, type MediaSessionBridge } from './lib/media-session';
  import { playlistMenuItems, trackMenuItems } from './lib/menu';
  import { SELECTION_CAP } from './lib/selection';
  import { LIBRARY_VIEW, sourceOf, viewCanSort, type View } from './lib/view';
  import { player } from './stores/player.svelte';
  import { ui } from './stores/ui.svelte';
  import { selection } from './stores/selection.svelte';
  import { library } from './stores/library.svelte';
  import type { SpectrumSource } from './lib/spectrum-source';

  const PAGE_SIZE = 100;
  const SEARCH_DEBOUNCE_MS = 120;
  const SEEK_STEP_MS = 5000;
  const VOLUME_STEP = 0.05;

  type Dialog =
    | { kind: 'createPlaylist'; seedIds?: string[] }
    | { kind: 'renamePlaylist'; playlist: PlaylistSummary }
    | { kind: 'deletePlaylist'; playlist: PlaylistSummary }
    | { kind: 'addToPlaylist'; ids: string[] }
    | { kind: 'removeTracks'; ids: string[] }
    | null;

  const listStore = new TrackWindow({
    pageSize: PAGE_SIZE,
    maxPages: 24,
    fetchPage: async (offset, limit) => {
      // parseTrackQuery：offset 模式的查询带 offset 覆盖，直接展开会被
      // 判别联合的 cursor 分支拒绝（旧 demo 未跑 svelte-check，遗留于此）
      // 失败直接抛：TrackWindow 自己会把列表切到"重试"态
      const page = await library.list(parseTrackQuery({ ...currentQuery(), offset, limit }));
      return { items: page.items, total: page.total };
    },
  });

  /* ---------------- store 的视图别名（模板沿用原名） ---------------- */

  let storeVersion = $state(0);
  let total = $state(0);
  let booted = $state(false);
  let loadingView = $state(false);
  let error = $state<string | null>(null);
  let toast = $state<string | null>(null);
  let restoreScrollTop = $state<number | null>(null);

  const view = $derived(ui.view);
  const search = $derived(ui.search);
  const sort = $derived(ui.sort);
  const direction = $derived(ui.direction);
  const artistId = $derived(ui.artistId);
  const albumId = $derived(ui.albumId);
  const settings = $derived(ui.settings);
  const playlists = $derived(ui.playlists);
  const diagnostics = $derived(ui.diagnostics);
  const settingsOpen = $derived(ui.settingsOpen);
  const sidePanel = $derived(ui.sidePanel);

  const playerState = $derived(player.state);
  const queueIds = $derived(player.queueIds);

  const scan = $derived(library.scan);
  const scanning = $derived(library.scanning);

  const selectionSet = $derived(selection.selection);
  const selectedIndex = $derived(selection.selectedIndex);

  let spectrumSource = $state<SpectrumSource | null>(null);

  let menu = $state<{ x: number; y: number; index: number; track: Track | null } | null>(null);
  let listMenu = $state<{ x: number; y: number; playlist: PlaylistSummary } | null>(null);
  let dialog = $state<Dialog>(null);

  /** 顶栏实例：快捷键 focusSearch 需要把焦点交给它的输入框 */
  let topBar = $state<{ focus: () => void } | null>(null);

  let mediaSession: MediaSessionBridge | null = null;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let lastScrollTop = 0;
  /** reload 的请求代次，用来丢弃过期响应 */
  let reloadGeneration = 0;

  /** UiState 节流器：scheduleUiSave 的定时职责独立出来，便于清理 */
  const uiTimer = {
    handle: undefined as ReturnType<typeof setTimeout> | undefined,
    clear() {
      clearTimeout(this.handle);
    },
    arm(fn: () => void) {
      this.handle = setTimeout(fn, 600);
    },
  };

  const bindings = $derived(resolveShortcutBindings(settings.shortcuts));
  const currentPlaylist = $derived(
    view.kind === 'playlist' ? (ui.playlistById(view.id) ?? null) : null,
  );
  const showPlays = $derived(view.kind === 'recent' || view.kind === 'mostPlayed');

  /* ---------------- 查询与列表 ---------------- */

  function currentQuery(): TrackQuery {
    return {
      mode: 'offset',
      offset: 0,
      limit: PAGE_SIZE,
      sort,
      direction,
      source: sourceOf(view),
      ...(search.trim() ? { search: search.trim() } : {}),
      ...(artistId ? { artistId } : {}),
      ...(albumId ? { albumId } : {}),
    };
  }

  async function reload(options: { restore?: boolean } = {}): Promise<void> {
    /*
     * 请求代次：快速点两个播放列表时，两次 library.list 会并发。
     * 先发的可能后回来，把列表覆盖成上一个视图的内容 —— 而标题已经是新视图了。
     * 只有最新一次的结果允许落到 store 上。
     */
    const generation = (reloadGeneration += 1);
    loadingView = true;
    error = null;
    try {
      const page = await library.list(currentQuery());
      if (generation !== reloadGeneration) return;
      listStore.reset(page.total);
      listStore.primePage(0, page.items);
      total = page.total;
      selection.clear();
      if (options.restore) restoreScrollTop = lastScrollTop;
      else restoreScrollTop = 0;
    } catch (err) {
      if (generation !== reloadGeneration) return;
      error = errorMessage(err);
    } finally {
      if (generation === reloadGeneration) loadingView = false;
      booted = true;
    }
  }

  function showToast(message: string): void {
    toast = message;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 2600);
  }

  /* ---------------- 视图切换 ---------------- */

  function setView(kind: 'library' | 'recent' | 'mostPlayed'): void {
    if (view.kind === kind) {
      void reload();
      return;
    }
    ui.view = { kind };
    void reload();
    ui.scheduleUiSave(() => lastScrollTop, uiTimer);
  }

  function openPlaylist(id: string): void {
    ui.view = { kind: 'playlist', id };
    void reload();
    ui.scheduleUiSave(() => lastScrollTop, uiTimer);
  }

  /* ---------------- 播放 ---------------- */

  /** 系统媒体中心：只订阅 PlayerState，与音频出口无关 */
  function buildMediaSession(): MediaSessionBridge {
    return createMediaSession(
      {
        toggle: () => player.toggle(),
        next: () => player.next(),
        prev: () => player.prev(),
        seek: (ms) => player.seek(ms),
        skip: (seconds) => player.seek((playerState?.positionMs ?? 0) + seconds * 1000),
        stop: () => player.clear(),
      },
      { actions: settings.mediaKeys },
    );
  }

  async function playIndex(index: number): Promise<void> {
    error = null;
    selection.clear();
    try {
      await player.playQuery(currentQuery(), index);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function playAll(): Promise<void> {
    if (total === 0) return;
    selection.clear();
    try {
      await player.playQuery(currentQuery(), 0);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function playQueueAt(index: number): Promise<void> {
    try {
      await player.playTracks(queueIds, index);
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function revealTrack(trackId: string | null): Promise<void> {
    if (!trackId) return;
    const revealed = await library.revealTrack(trackId);
    if (revealed && !revealed.revealed) error = revealed.reason ?? '无法定位文件';
  }

  /* ---------------- 选择 ---------------- */

  function handleSelect(index: number, event: MouseEvent | KeyboardEvent): void {
    if (selection.handleSelect(index, event) === 'capped') {
      showToast(`单次最多选择 ${SELECTION_CAP} 首，已截取刚点中的一段`);
    }
  }

  function clearSelection(): void {
    selection.clear();
  }

  async function resolveSelectionIds(): Promise<string[]> {
    if (selection.size === 0) return [];
    try {
      return await library.idsForSelection(currentQuery(), selection.sortedIndices());
    } catch (err) {
      error = errorMessage(err);
      return [];
    }
  }

  async function batchEnqueue(): Promise<void> {
    const ids = await resolveSelectionIds();
    if (ids.length === 0) return;
    try {
      await player.enqueue(ids);
      showToast(`已加入队列 ${ids.length} 首`);
      selection.clear();
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function batchPlay(): Promise<void> {
    const ids = await resolveSelectionIds();
    if (ids.length === 0) return;
    try {
      await player.playTracks(ids, 0);
      selection.clear();
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function batchAddToPlaylist(playlistId: string): Promise<void> {
    const ids = await resolveSelectionIds();
    dialog = null;
    if (ids.length === 0) return;
    try {
      const added = await ui.addTracks(playlistId, ids);
      await ui.refreshPlaylists();
      showToast(`新增 ${added.added} 首，跳过 ${added.skipped} 首`);
      selection.clear();
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function removeTracksByIds(ids: string[]): Promise<void> {
    if (ids.length === 0) return;
    dialog = null;
    try {
      const removed = await library.removeTracks(ids);
      showToast(`已移出曲库 ${removed} 首`);
      selection.clear();
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await library.loadStats();
    await ui.refreshPlaylists();
    await reload({ restore: true });
  }

  /* ---------------- 播放列表 ---------------- */

  function openCreatePlaylist(seedIds?: string[]): void {
    dialog = { kind: 'createPlaylist', ...(seedIds ? { seedIds } : {}) };
  }

  async function createPlaylistConfirmed(name: string): Promise<void> {
    const current = dialog;
    dialog = null;
    const seed = current?.kind === 'createPlaylist' ? current.seedIds : undefined;
    let id: string;
    try {
      id = await ui.createPlaylist(name, seed);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await ui.refreshPlaylists();
    openPlaylist(id);
  }

  async function renamePlaylistConfirmed(playlist: PlaylistSummary, name: string): Promise<void> {
    dialog = null;
    try {
      await ui.renamePlaylist(playlist.id, name);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await ui.refreshPlaylists();
  }

  async function deletePlaylist(playlist: PlaylistSummary): Promise<void> {
    dialog = null;
    try {
      await ui.removePlaylist(playlist.id);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await ui.refreshPlaylists();
    if (view.kind === 'playlist' && view.id === playlist.id) {
      ui.view = LIBRARY_VIEW;
      await reload();
    }
    showToast(`已删除「${playlist.name}」`);
  }

  async function clearPlaylistItems(playlist: PlaylistSummary): Promise<void> {
    try {
      await ui.clearPlaylist(playlist.id);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await ui.refreshPlaylists();
    if (view.kind === 'playlist' && view.id === playlist.id) await reload();
    showToast('已清空播放列表');
  }

  async function addTracksToPlaylistById(playlistId: string, ids: string[]): Promise<void> {
    dialog = null;
    if (ids.length === 0) return;
    try {
      const added = await ui.addTracks(playlistId, ids);
      await ui.refreshPlaylists();
      showToast(`新增 ${added.added} 首，跳过 ${added.skipped} 首`);
      selection.clear();
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    if (view.kind === 'playlist' && view.id === playlistId) await reload({ restore: true });
  }

  async function playPlaylist(playlist: PlaylistSummary): Promise<void> {
    ui.view = { kind: 'playlist', id: playlist.id };
    await reload();
    await playAll();
  }

  async function clearHistory(): Promise<void> {
    try {
      await library.clearHistory();
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    showToast('播放历史已清空');
    await library.loadStats();
    if (view.kind === 'recent' || view.kind === 'mostPlayed') await reload();
    if (settingsOpen) await ui.refreshDiagnostics();
  }

  async function movePlaylistOrder(playlist: PlaylistSummary, delta: number): Promise<void> {
    const index = playlists.findIndex((item) => item.id === playlist.id);
    if (index < 0) return;
    const target = index + delta;
    if (target < 0 || target >= playlists.length) return;
    await ui.reorderPlaylists(index, target);
  }

  async function movePlaylistItem(from: number, to: number): Promise<void> {
    if (view.kind !== 'playlist') return;
    try {
      await ui.moveItem(view.id, from, to);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await reload({ restore: true });
  }

  async function removePlaylistItemAt(index: number): Promise<void> {
    if (view.kind !== 'playlist') return;
    try {
      await ui.removeItemAt(view.id, index);
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await ui.refreshPlaylists();
    await reload({ restore: true });
  }

  /* ---------------- 搜索与排序 ---------------- */

  function onSearch(value: string): void {
    ui.search = value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void reload(), SEARCH_DEBOUNCE_MS);
  }

  function clearSearch(): void {
    ui.search = '';
    void reload();
  }

  function changeSort(field: TrackSort): void {
    if (sort === field) ui.direction = direction === 'asc' ? 'desc' : 'asc';
    else {
      ui.sort = field;
      ui.direction = 'asc';
    }
    void reload();
  }

  function clearFilter(): void {
    ui.artistId = null;
    ui.albumId = null;
    void reload();
  }

  /* ---------------- 设置 ---------------- */

  function applySettings(next: AppSettings): void {
    ui.applySettings(next, (message) => (error = message));
  }

  /* ---------------- 视图状态 ---------------- */

  function scheduleUiSave(): void {
    ui.scheduleUiSave(
      () => lastScrollTop,
      uiTimer,
    );
  }

  function onListScroll(scrollTop: number): void {
    lastScrollTop = scrollTop;
    scheduleUiSave();
  }

  /* ---------------- 面板 ---------------- */

  function togglePanel(panel: Exclude<NonNullable<typeof ui.sidePanel>, null>): void {
    ui.sidePanel = ui.sidePanel === panel ? null : panel;
    scheduleUiSave();
    if (ui.sidePanel === 'lyrics') {
      const trackId = playerState?.current?.id ?? null;
      if (trackId) ui.loadLyrics(trackId);
    }
    if (ui.sidePanel === 'spectrum') ensureSpectrumSource();
  }

  /** 频谱数据源只在面板打开时创建：建图 = 音频路径切换，不要提前 */
  function ensureSpectrumSource(): void {
    if (ui.sidePanel !== 'spectrum') {
      spectrumSource = null;
      return;
    }
    try {
      spectrumSource = player.getSpectrumSource();
    } catch {
      spectrumSource = null;
    }
  }

  // 设置里关掉频谱时，若面板正开着就一并收起（源已无意义）
  $effect(() => {
    if (!settings.spectrum && ui.sidePanel === 'spectrum') {
      ui.sidePanel = null;
      spectrumSource = null;
    }
  });

  /* ---------------- 右键菜单 ---------------- */

  function openMenu(event: MouseEvent, index: number, track: Track | null): void {
    menu = { x: event.clientX, y: event.clientY, index, track };
  }

  function openPlaylistMenu(event: MouseEvent, playlist: PlaylistSummary): void {
    listMenu = { x: event.clientX, y: event.clientY, playlist };
  }

  /* ---------------- 键盘 ---------------- */

  function isTyping(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    if (!el) return false;
    return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable;
  }

  function runAction(action: ShortcutAction): void {
    switch (action) {
      case 'playPause':
        player.toggle();
        return;
      case 'next':
        player.next();
        return;
      case 'prev':
        player.prev();
        return;
      case 'seekForward':
        player.seek((playerState?.positionMs ?? 0) + SEEK_STEP_MS);
        return;
      case 'seekBackward':
        player.seek((playerState?.positionMs ?? 0) - SEEK_STEP_MS);
        return;
      case 'volumeUp':
        player.setVolume((playerState?.volume ?? 0.85) + VOLUME_STEP);
        return;
      case 'volumeDown':
        player.setVolume((playerState?.volume ?? 0.85) - VOLUME_STEP);
        return;
      case 'rateUp':
        player.setRate((playerState?.rate ?? 1) + AUDIO_RATE_STEP);
        return;
      case 'rateDown':
        player.setRate((playerState?.rate ?? 1) - AUDIO_RATE_STEP);
        return;
      case 'mute':
        player.toggleMute();
        return;
      case 'focusSearch':
        topBar?.focus();
        return;
      case 'toggleQueue':
        togglePanel('queue');
        return;
      case 'toggleLyrics':
        togglePanel('lyrics');
        return;
      case 'toggleShuffle':
        player.setShuffle(!(playerState?.shuffle ?? false));
        return;
      case 'cycleRepeat':
        player.cycleRepeat();
        return;
      case 'selectAll':
        selection.selectAll(total);
        return;
      case 'openSettings':
        void ui.openSettings();
        return;
    }
  }

  function onKeydown(event: KeyboardEvent): void {
    if (isTyping(event.target)) {
      if (event.key === 'Escape') (event.target as HTMLElement).blur();
      return;
    }

    const intent = listKeyIntent(event, {
      total,
      selectedIndex,
      hasSelection: selection.size > 0,
      hasPanel: sidePanel !== null,
      modalOpen: settingsOpen || dialog !== null || menu !== null || listMenu !== null,
      bindings,
    });
    if (!intent) return;

    switch (intent.kind) {
      case 'move':
        event.preventDefault();
        selection.selectedIndex = intent.index;
        return;
      case 'play':
        event.preventDefault();
        void playIndex(intent.index);
        return;
      case 'clearSelection':
        selection.clear();
        return;
      case 'closePanel':
        ui.sidePanel = null;
        return;
      case 'action':
        event.preventDefault();
        runAction(intent.action);
        return;
    }
  }

  /* ---------------- 扫描 ---------------- */

  async function chooseRoot(): Promise<void> {
    error = null;
    try {
      const path = await library.chooseRoot();
      if (path) await startScan();
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function startScan(): Promise<void> {
    error = null;
    try {
      await library.startScan();
    } catch (err) {
      error = errorMessage(err);
      return;
    }
    await library.loadStats();
    await ui.refreshPlaylists();
    await reload({ restore: true });
  }

  /* ---------------- 副作用 ---------------- */

  // 主题与密度：写在 <html> 的 data 属性上，CSS 变量随之切换，无需重建组件
  $effect(() => {
    document.documentElement.dataset.theme = settings.theme;
    document.documentElement.dataset.density = settings.density;
  });

  // 封面主色：切歌或开关变化时重新提取；异步结果带取消标记，防止旧封面覆盖新颜色
  $effect(() => {
    const artworkId = playerState?.current?.artworkId ?? null;
    if (!settings.accentFromArtwork || !artworkId) {
      document.documentElement.style.removeProperty('--accent');
      document.documentElement.style.removeProperty('--accent-soft');
      return;
    }

    let cancelled = false;
    const url = artworkUrl(artworkId, 96);
    if (!url) return;
    void accentForArtwork(url).then((hex) => {
      if (cancelled || !hex) return;
      document.documentElement.style.setProperty('--accent', hex);
      document.documentElement.style.setProperty('--accent-soft', `${hex}33`);
    });
    return () => {
      cancelled = true;
    };
  });

  // ReplayGain 归一化**不在这里做**：增益在 Rust DSP 链内应用，
  // 开关由 `app_set_settings` 的设置标志驱动（src-tauri/src/lib.rs 的
  // 事件泵每拍读 normalizeVolume）。旧 UI 的 setNormalization 早就是 no-op。

  /**
   * 系统媒体键开关。
   *
   * Media Session 的动作处理器只能在创建时注册，所以切换开关要重建一次：
   * dispose 会清掉旧处理器，再按新开关注册。`mediaSessionActions` 记录
   * 当前生效值，避免每次设置对象变化都重建（比如改主题）。
   */
  let mediaSessionActions: boolean | null = null;
  $effect(() => {
    const enabled = settings.mediaKeys;
    if (mediaSessionActions === null || mediaSessionActions === enabled) return;
    mediaSessionActions = enabled;
    mediaSession?.dispose();
    mediaSession = buildMediaSession();
    mediaSession.update(playerState);
  });

  // 歌词面板打开且换歌时重新取词
  $effect(() => {
    const trackId = playerState?.current?.id ?? null;
    if (sidePanel !== 'lyrics' || !trackId) return;
    ui.loadLyrics(trackId);
  });

  onMount(() => {
    const unsubscribers: Array<() => void> = [];
    // player:state 事件已到过的标志：晚到的 fetchState 结果不能盖回旧状态
    let stateEventSeen = false;

    unsubscribers.push(
      listStore.subscribe(() => {
        storeVersion += 1;
        total = listStore.total;
      }),
    );

    unsubscribers.push(
      player.init({
        onState: (next) => {
          stateEventSeen = true;
          mediaSession?.update(next);
        },
      }),
    );

    mediaSession = buildMediaSession();
    mediaSessionActions = settings.mediaKeys;

    unsubscribers.push(library.watchScan(() => {
      void library.loadStats();
      void ui.refreshPlaylists();
      void reload({ restore: true });
    }));

    void (async () => {
      const [dataDirRestored, playerResult] = await Promise.all([
        library.fetchInfo(),
        player.fetchState(),
      ]);
      const lastScrollTopRestored = await ui.bootstrap();

      if (dataDirRestored) library.dataDir = dataDirRestored;
      lastScrollTop = lastScrollTopRestored;
      // 恢复的频谱面板要重新取数据源（源不可序列化）
      if (sidePanel === 'spectrum' && settings.spectrum) ensureSpectrumSource();

      await library.loadStats();
      await reload({ restore: true });

      // fetchState 已解包信封，直接就是 PlayerState | null。
      // 旧代码误用 .ok/.data，条件恒假 —— 启动恢复块从未执行过，
      // 一直被 init() 里的 onState 订阅掩盖。
      // 另外 fetchState 与 player:state 事件并发：事件已先到（stateEventSeen）
      // 时 store 里已是新状态，丢弃这份晚到的快照，避免盖回旧值。
      if (playerResult && !stateEventSeen) {
        player.state = playerResult;
        mediaSession?.update(playerResult);
        // 装载由主进程补发的 load 指令完成（attach 时一次性发出）；
        // 音量/静音/归一化全部以后端状态为准，UI 不再"对齐"一遍
        // （旧的 alignVolume/setNormalization 在远程出口上本来就是 no-op）
        await player.refreshQueue(playerResult.queueVersion);
      }
    })();

    return () => {
      for (const unsubscribe of unsubscribers) unsubscribe();
      clearTimeout(searchTimer);
      clearTimeout(toastTimer);
      uiTimer.clear();
      mediaSession?.dispose();
    };
  });

</script>

<svelte:window onkeydown={onKeydown} />

<AppShell
  onview={setView}
  onplaylist={openPlaylist}
  oncreate={() => openCreatePlaylist()}
  onplaylistmenu={openPlaylistMenu}
  onqueueplay={(index) => void playQueueAt(index)}
  onbatchplay={() => void batchPlay()}
  onbatchenqueue={() => void batchEnqueue()}
  onbatchaddto={(playlistId) => void batchAddToPlaylist(playlistId)}
  onbatchremove={async () => {
    const ids = await resolveSelectionIds();
    if (ids.length > 0) dialog = { kind: 'removeTracks', ids };
  }}
  ontogglepanel={togglePanel}
  onsynced={() => void reload()}
  {spectrumSource}
>
  <TopBar
    bind:this={topBar}
    {search}
    {sort}
    {direction}
    canSort={viewCanSort(view)}
    {total}
    {scanning}
    onsearch={onSearch}
    onclearsearch={clearSearch}
    onsort={() => changeSort('added')}
    onsettings={() => void ui.openSettings()}
    onchooseroot={chooseRoot}
    onscan={startScan}
    onimport={() => togglePanel('import')}
  />

  <ViewHeader
    {view}
    playlist={currentPlaylist}
    {total}
    onplayall={playAll}
    onrename={() => {
      if (currentPlaylist) dialog = { kind: 'renamePlaylist', playlist: currentPlaylist };
    }}
    onclearitems={() => {
      if (currentPlaylist) void clearPlaylistItems(currentPlaylist);
    }}
    ondelete={() => {
      if (currentPlaylist) dialog = { kind: 'deletePlaylist', playlist: currentPlaylist };
    }}
    onclearhistory={() => void clearHistory()}
  />

  {#if artistId || albumId}
    <div class="filters">
      {#if artistId}<span class="filter">艺人筛选 <button class="link" onclick={clearFilter}>清除</button></span>{/if}
      {#if albumId}<span class="filter">专辑筛选 <button class="link" onclick={clearFilter}>清除</button></span>{/if}
    </div>
  {/if}

  {#if scan && scanning}
    <div class="scan-bar">
      <div class="progress">
        <i style={`width:${scan.total ? Math.min(100, Math.round((scan.scanned / scan.total) * 100)) : 8}%`}></i>
      </div>
      <span>
        {scan.phase === 'walking'
          ? '正在遍历目录…'
          : `${scan.scanned}/${scan.total ?? '?'} · 新增 ${scan.imported} · 跳过 ${scan.skipped} · 失败 ${scan.failed}`}
      </span>
      <button class="btn" onclick={() => library.cancelScan()}>取消</button>
    </div>
  {/if}

  {#if error}
    <div class="error" aria-live="assertive">
      {error}
      <button class="link" onclick={() => (error = null)}>关闭</button>
    </div>
  {/if}

  {#if playerState?.status === 'error' && playerState.error}
    <div class="error">
      《{playerState.current?.title ?? '未知曲目'}》{playerState.error.message}
      <button class="link" onclick={() => player.next()}>跳到下一首</button>
    </div>
  {/if}

  {#if total > 0}
    <TrackList
      store={listStore}
      version={storeVersion}
      {total}
      query={search}
      {sort}
      {direction}
      canSort={viewCanSort(view)}
      {showPlays}
      onsort={(field) => changeSort(field)}
      currentId={playerState?.current?.id ?? null}
      {selectedIndex}
      selection={selectionSet}
      {restoreScrollTop}
      rowHeight={settings.density === 'compact' ? 42 : 52}
      playing={playerState?.status === 'playing'}
      onplay={(index) => void playIndex(index)}
      onselect={handleSelect}
      onmenu={openMenu}
      onscroll={onListScroll}
      onrestored={() => (restoreScrollTop = null)}
      onretry={() => void reload()}
      onmove={view.kind === 'playlist' ? (from, to) => void movePlaylistItem(from, to) : undefined}
    />
  {:else if booted}
    <div class="empty">
      {#if search.trim()}
        <strong>没有匹配的曲目</strong>
        <span>换个关键词，或清除搜索条件</span>
        <button class="btn" onclick={() => { ui.search = ''; void reload(); }}>清除搜索</button>
      {:else if view.kind === 'recent'}
        <strong>还没有播放记录</strong>
        <span>听几首歌，这里会按时间倒序记录</span>
      {:else if view.kind === 'mostPlayed'}
        <strong>还没有常听曲目</strong>
        <span>播放次数会从零开始积累</span>
      {:else if view.kind === 'playlist'}
        <strong>这个播放列表还是空的</strong>
        <span>在曲库里选中歌曲，右键或批量操作加入这里</span>
        <button class="btn" onclick={() => setView('library')}>去曲库看看</button>
      {:else}
        <strong>曲库还是空的</strong>
        <span>选择一个音乐目录，AIYA 会扫描并建立索引</span>
        <button class="btn primary" onclick={chooseRoot} disabled={scanning}>选择音乐目录</button>
      {/if}
    </div>
  {:else}
    <div class="empty"><strong>正在启动…</strong></div>
  {/if}
</AppShell>

{#if toast}
  <div class="toast" aria-live="polite">{toast}</div>
{/if}

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    onclose={() => (menu = null)}
    items={trackMenuItems(menu.track, view, {
      play: () => void playIndex(menu?.index ?? 0),
      insertNext: () => {
        const trackId = menu?.track?.id;
        if (trackId) player.insertNext(trackId);
      },
      enqueue: () => {
        const trackId = menu?.track?.id;
        if (trackId) void player.enqueue([trackId]);
      },
      addToPlaylist: () => {
        const trackId = menu?.track?.id;
        if (trackId) dialog = { kind: 'addToPlaylist', ids: [trackId] };
      },
      filterByArtist: () => {
        const id = menu?.track?.artistId;
        if (!id) return;
        ui.artistId = id;
        void reload();
      },
      filterByAlbum: () => {
        const id = menu?.track?.albumId;
        if (!id) return;
        ui.albumId = id;
        void reload();
      },
      reveal: () => void revealTrack(menu?.track?.id ?? null),
      remove: () => {
        const trackId = menu?.track?.id;
        if (view.kind === 'playlist') void removePlaylistItemAt(menu?.index ?? -1);
        else if (trackId) dialog = { kind: 'removeTracks', ids: [trackId] };
      },
    })}
  />
{/if}

{#if listMenu}
  <ContextMenu
    x={listMenu.x}
    y={listMenu.y}
    onclose={() => (listMenu = null)}
    items={playlistMenuItems(listMenu.playlist, {
      index: playlists.findIndex((item) => item.id === listMenu?.playlist.id),
      total: playlists.length,
    }, {
      play: () => void playPlaylist(listMenu!.playlist),
      rename: () => {
        dialog = { kind: 'renamePlaylist', playlist: listMenu!.playlist };
      },
      moveUp: () => void movePlaylistOrder(listMenu!.playlist, -1),
      moveDown: () => void movePlaylistOrder(listMenu!.playlist, 1),
      clear: () => void clearPlaylistItems(listMenu!.playlist),
      remove: () => {
        dialog = { kind: 'deletePlaylist', playlist: listMenu!.playlist };
      },
    })}
  />
{/if}

{#if dialog?.kind === 'createPlaylist'}
  <PromptDialog
    title="新建播放列表"
    label="名称"
    confirmText="创建"
    onconfirm={(name) => void createPlaylistConfirmed(name)}
    oncancel={() => (dialog = null)}
  />
{:else if dialog?.kind === 'renamePlaylist'}
  <PromptDialog
    title="重命名播放列表"
    label="名称"
    value={dialog.playlist.name}
    confirmText="保存"
    onconfirm={(name) => {
      if (dialog?.kind === 'renamePlaylist') void renamePlaylistConfirmed(dialog.playlist, name);
    }}
    oncancel={() => (dialog = null)}
  />
{:else if dialog?.kind === 'deletePlaylist'}
  <ConfirmDialog
    title="删除播放列表"
    message={`「${dialog.playlist.name}」会被删除，曲库里的歌曲不受影响。`}
    confirmText="删除"
    danger
    onconfirm={() => {
      if (dialog?.kind === 'deletePlaylist') void deletePlaylist(dialog.playlist);
    }}
    oncancel={() => (dialog = null)}
  />
{:else if dialog?.kind === 'removeTracks'}
  <ConfirmDialog
    title="从曲库移除"
    message={`将从曲库移除 ${dialog.ids.length} 首曲目。磁盘上的文件不会被删除，下次扫描会重新导入。`}
    confirmText="移除"
    danger
    onconfirm={() => {
      if (dialog?.kind === 'removeTracks') void removeTracksByIds(dialog.ids);
    }}
    oncancel={() => (dialog = null)}
  />
{:else if dialog?.kind === 'addToPlaylist'}
  <PlaylistPicker
    title="添加到播放列表"
    count={dialog.ids.length}
    {playlists}
    onpick={(playlistId) => {
      if (dialog?.kind === 'addToPlaylist') void addTracksToPlaylistById(playlistId, dialog.ids);
    }}
    oncreate={() => {
      if (dialog?.kind === 'addToPlaylist') openCreatePlaylist(dialog.ids);
    }}
    onclose={() => (dialog = null)}
  />
{/if}

{#if settingsOpen}
  <SettingsPanel
    {settings}
    {diagnostics}
    beets={ui.beets}
    onclose={() => (ui.settingsOpen = false)}
    onsave={applySettings}
    onrefresh={() => void ui.refreshDiagnostics()}
    onclearhistory={() => void clearHistory()}
    onopendatadir={() => ui.openDataDir()}
  />
{/if}

