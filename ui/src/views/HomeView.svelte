<script lang="ts">
  import { ArrowRight, Clock3, Flame, LibraryBig, ListMusic, Sparkles } from '@lucide/svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import InlineError from '../components/ui/InlineError.svelte';
  import TrackTable from '../components/TrackTable.svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSource } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { errorMessage } from '../lib/errors';
  import { batch, type HomeSection } from '../stores/batch.svelte';
  import { library } from '../stores/library.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';

  /**
   * 首页：歌单宫格 + 三段曲目预览（最近播放 / 常听 / 最近添加）。
   *
   * 三段都走 `TrackTable`（真表头 + 勾选列 + 三态全选），所以首页也能
   * **批量管理**：选中 → 底部批量条（播放 / 加入队列 / 加入歌单 / 移出曲库）。
   * 选择集按 ID，三段之间互斥（在 A 段选会清掉 B 段的选中）——
   * 这套状态与批量动作在 `stores/batch.svelte.ts`（B4-2b 单点化，
   * 与列表页共用同一个批量条与同一份动作实现）。
   */
  const SECTION_SIZE = 8;

  let recent = $state<TrackRow[]>([]);
  let frequent = $state<TrackRow[]>([]);
  let added = $state<TrackRow[]>([]);
  let loading = $state(true);
  let loadError = $state('');

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
</script>

<div class="page-scroll home-page">
  <Header
    kicker="YOUR LIBRARY"
    title="首页"
    subtitle="回到常听的音乐，或继续整理你的本地曲库。"
  >
    {#snippet actions()}
      <Button onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>
        浏览全部曲目
        {#snippet icon()}<ArrowRight size={14} />{/snippet}
      </Button>
    {/snippet}
  </Header>

  {#if loadError}<InlineError message={loadError} ondismiss={() => (loadError = '')} />{/if}

  <section class="home-section">
    <div class="section-heading">
      <div><ListMusic size={17} /><h2>歌单</h2></div>
      <span>{playlists.items.length} 个</span>
    </div>
    <div class="playlist-grid">
      <button class="playlist-card library-card" onclick={() => nav.go({ kind: 'builtin', view: 'library' })}>
        <span class="playlist-card-icon"><LibraryBig size={23} /></span>
        <strong>全部曲目</strong>
        <span>{library.stats?.tracks ?? 0} 首</span>
      </button>
      {#each playlists.items as playlist (playlist.id)}
        <button class="playlist-card" onclick={() => nav.go({ kind: 'playlist', id: playlist.id })}>
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
      <button class="section-link" onclick={() => nav.go({ kind: 'builtin', view: 'recent' })}>查看全部 <ArrowRight size={13} /></button>
    </div>
    <TrackTable
      rows={recent}
      {loading}
      empty="还没有播放记录，听几首歌后会出现在这里。"
      valueKind="duration"
      selected={batch.selectedIn('recent')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => batch.setSection('recent', ids)}
      onplay={(index) => void play(recent, index)}
    />
  </section>

  <section class="home-section">
    <div class="section-heading">
      <div><Flame size={17} /><h2>常听</h2></div>
      <button class="section-link" onclick={() => nav.go({ kind: 'builtin', view: 'mostPlayed' })}>查看全部 <ArrowRight size={13} /></button>
    </div>
    <TrackTable
      rows={frequent}
      {loading}
      empty="播放次数积累后，这里会形成你的常听榜单。"
      valueKind="plays"
      selected={batch.selectedIn('frequent')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => batch.setSection('frequent', ids)}
      onplay={(index) => void play(frequent, index)}
    />
  </section>

  <section class="home-section">
    <div class="section-heading">
      <div><Sparkles size={17} /><h2>最近添加</h2></div>
      <button class="section-link" onclick={() => nav.go({ kind: 'builtin', view: 'library', sort: { field: 'added', direction: 'desc' } })}>
        查看全部 <ArrowRight size={13} />
      </button>
    </div>
    <TrackTable
      rows={added}
      {loading}
      empty="曲库还是空的，到设置页选择目录并扫描。"
      valueKind="added"
      selected={batch.selectedIn('added')}
      currentId={player.state?.current?.id ?? null}
      onchange={(ids) => batch.setSection('added', ids)}
      onplay={(index) => void play(added, index)}
    />
  </section>
</div>
