<script lang="ts">
  import { ArrowRight, Clock3, Flame, LibraryBig, ListMusic, Sparkles } from '@lucide/svelte';
  import BatchBar from '../components/BatchBar.svelte';
  import TrackTable from '../components/TrackTable.svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSource } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { errorMessage } from '../lib/errors';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 首页：歌单宫格 + 三段曲目预览（最近播放 / 常听 / 最近添加）。
   *
   * 三段都走 `TrackTable`（真表头 + 勾选列 + 三态全选），所以首页也能
   * **批量管理**：选中 → 底部批量条（播放 / 加入队列 / 加入歌单 / 移出曲库）。
   * 选择集按 ID，三段之间互斥（在 A 段选会清掉 B 段的选中）。
   */
  const SECTION_SIZE = 8;

  let recent = $state<TrackRow[]>([]);
  let frequent = $state<TrackRow[]>([]);
  let added = $state<TrackRow[]>([]);
  let loading = $state(true);
  let loadError = $state('');

  /** 当前有选中的区块 + 选中 ID（一次只有一段生效） */
  let selSection = $state<'recent' | 'frequent' | 'added' | null>(null);
  let selIds = $state<Set<string>>(new Set());
  const selList = $derived([...selIds]);
  const EMPTY: ReadonlySet<string> = new Set();

  function selectedIn(section: 'recent' | 'frequent' | 'added'): ReadonlySet<string> {
    return selSection === section ? selIds : EMPTY;
  }

  function onChange(section: 'recent' | 'frequent' | 'added', ids: Set<string>): void {
    // 换区块就把上一段的选择清空：批量条只有一个，不能同时代表两段
    if (ids.size === 0) {
      selSection = null;
      selIds = new Set();
      return;
    }
    selSection = section;
    selIds = ids;
  }

  function clearSelection(): void {
    selSection = null;
    selIds = new Set();
  }

  async function loadSection(source: TrackSource, recentlyAdded = false): Promise<TrackRow[]> {
    const page = await library.list({
      mode: 'offset',
      offset: 0,
      limit: SECTION_SIZE,
      source,
      ...(recentlyAdded ? { sort: 'added' as const, direction: 'desc' as const } : {}),
    });
    return page.items;
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = '';
    try {
      [recent, frequent, added] = await Promise.all([
        loadSection({ kind: 'recent' }),
        loadSection({ kind: 'mostPlayed' }),
        loadSection({ kind: 'library' }, true),
      ]);
    } catch (error) {
      loadError = errorMessage(error);
    } finally {
      loading = false;
    }
  }

  /**
   * 曲库总数变化时重载（扫描完成、批量移出曲库都会改它）。
   * 不在 `onMount` 里自己发一次：`boot` 之后 `library.stats` 才就位，
   * 提前查会在"曲库还没准备好"的时刻白跑三条查询。
   */
  let lastTotal: number | null = null;
  $effect(() => {
    const total = library.stats?.tracks ?? null;
    if (total === null || total === lastTotal) return;
    lastTotal = total;
    void load();
  });

  async function play(rows: TrackRow[], index: number): Promise<void> {
    try {
      await player.playTracks(rows.map((row) => row.id), index);
    } catch (error) {
      notify.showError(errorMessage(error));
    }
  }

  /* ---------------- 批量操作（选择集已是 ID，不需要后端换算） ---------------- */

  async function batchPlay(): Promise<void> {
    if (selList.length === 0) return;
    try {
      await player.playTracks(selList, 0);
      clearSelection();
    } catch (error) {
      notify.showError(errorMessage(error));
    }
  }

  async function batchEnqueue(): Promise<void> {
    if (selList.length === 0) return;
    try {
      await player.enqueue(selList);
      notify.showToast(`已加入队列 ${selList.length} 首`);
      clearSelection();
    } catch (error) {
      notify.showError(errorMessage(error));
    }
  }

  async function batchAddTo(playlistId: string): Promise<void> {
    if (selList.length === 0) return;
    await playlists.addTracks(playlistId, selList);
    clearSelection();
  }

  function batchRemove(): void {
    if (selList.length === 0) return;
    overlay.dialog = { kind: 'removeTracks', ids: selList };
    clearSelection();
  }
</script>

<div class="page-scroll home-page">
  <header class="page-heading">
    <div>
      <span class="page-kicker">YOUR LIBRARY</span>
      <h1>首页</h1>
      <p>回到常听的音乐，或继续整理你的本地曲库。</p>
    </div>
    <button class="btn" onclick={() => list.openBuiltinView('library')}>
      浏览全部曲目 <ArrowRight size={14} />
    </button>
  </header>

  {#if loadError}<div class="error">{loadError}</div>{/if}

  <section class="home-section">
    <div class="section-heading">
      <div><ListMusic size={17} /><h2>歌单</h2></div>
      <span>{ui.playlists.length} 个</span>
    </div>
    <div class="playlist-grid">
      <button class="playlist-card library-card" onclick={() => list.openBuiltinView('library')}>
        <span class="playlist-card-icon"><LibraryBig size={23} /></span>
        <strong>全部曲目</strong>
        <span>{library.stats?.tracks ?? 0} 首</span>
      </button>
      {#each ui.playlists as playlist (playlist.id)}
        <button class="playlist-card" onclick={() => list.openPlaylist(playlist.id)}>
          {#if playlist.artworkId}
            <img src={artworkUrl(playlist.artworkId, 160) ?? ''} alt="" loading="lazy" />
          {:else}
            <span class="playlist-card-icon"><ListMusic size={23} /></span>
          {/if}
          <strong>{playlist.name}</strong>
          <span>{playlist.trackCount} 首</span>
        </button>
      {:else}
        <div class="playlist-empty">从曲目右键菜单或批量操作创建歌单。</div>
      {/each}
    </div>
  </section>

  <section class="home-section">
    <div class="section-heading">
      <div><Clock3 size={17} /><h2>最近播放</h2></div>
      <button class="section-link" onclick={() => list.openBuiltinView('recent')}>查看全部 <ArrowRight size={13} /></button>
    </div>
    <TrackTable
      rows={recent}
      {loading}
      empty="还没有播放记录，听几首歌后会出现在这里。"
      valueKind="duration"
      selected={selectedIn('recent')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => onChange('recent', ids)}
      onplay={(index) => void play(recent, index)}
    />
  </section>

  <section class="home-section">
    <div class="section-heading">
      <div><Flame size={17} /><h2>常听</h2></div>
      <button class="section-link" onclick={() => list.openBuiltinView('mostPlayed')}>查看全部 <ArrowRight size={13} /></button>
    </div>
    <TrackTable
      rows={frequent}
      {loading}
      empty="播放次数积累后，这里会形成你的常听榜单。"
      valueKind="plays"
      selected={selectedIn('frequent')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => onChange('frequent', ids)}
      onplay={(index) => void play(frequent, index)}
    />
  </section>

  <section class="home-section">
    <div class="section-heading">
      <div><Sparkles size={17} /><h2>最近添加</h2></div>
      <button class="section-link" onclick={() => list.openBuiltinView('library', { field: 'added', direction: 'desc' })}>
        查看全部 <ArrowRight size={13} />
      </button>
    </div>
    <TrackTable
      rows={added}
      {loading}
      empty="曲库还是空的，到设置页选择目录并扫描。"
      valueKind="added"
      selected={selectedIn('added')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => onChange('added', ids)}
      onplay={(index) => void play(added, index)}
    />
  </section>
</div>

{#if selList.length > 0}
  <BatchBar
    count={selList.length}
    playlists={ui.playlists}
    onplay={() => void batchPlay()}
    onenqueue={() => void batchEnqueue()}
    onaddto={(playlistId) => void batchAddTo(playlistId)}
    onremove={batchRemove}
    onclear={clearSelection}
  />
{/if}
