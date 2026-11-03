<script lang="ts">
  import { ListMusic, Play } from '@lucide/svelte';
  import TrackTable from '../components/TrackTable.svelte';
  import InlineError from '../components/ui/InlineError.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import { ipc } from '../lib/ipc';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSource } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { errorMessage } from '../lib/errors';
  import { formatDurationLong, formatPlays } from '../lib/format';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';

  /**
   * 推荐页：仪表盘形态，三行布局。
   *
   * - 第一行 歌单卡片行（约 70%）+ 统计小卡（约 30%）：封面即播放键
   *   （点击直接起播，不跳页），卡片名点击才进歌单视图；统计卡只陈述
   *   事实（曲库规模与收听累计），**不可点**——一级导航在侧栏，卡片
   *   变按钮就是二次修订撤销过的视觉冗余。
   * - 第二行 双表（常听 | 最近播放）：同一 TrackTable 组件、同列数，
   *   一个回答"你最常听什么"，一个回答"你刚听过什么"，数据源同为
   *   播放历史，放一行成对出现。
   * - 第三行 最近添加：整页宽单列表（多一列专辑），新入库曲目的出口。
   *
   * 不设页标题（Header）与「全部曲目」路由卡：侧栏已是全局导航。
   * 「最近播放 / 最近添加」在曲库页也有 tab，首页给的是免切换的快照。
   */
  const FREQUENT_SIZE = 15;
  const ADDED_SIZE = 10;

  let frequent = $state<TrackRow[]>([]);
  let recentPlayed = $state<TrackRow[]>([]);
  let recentAdded = $state<TrackRow[]>([]);
  /** 统计卡的数字（HistoryStats 里只取两个），null = 还没加载到 */
  let playStats = $state<{ totalPlays: number; totalMs: number } | null>(null);
  let loading = $state(true);
  let loadError = $state('');

  async function loadSection(
    source: TrackSource,
    limit: number,
    sort: 'title' | 'added' = 'title',
    direction: 'asc' | 'desc' = 'asc',
  ): Promise<TrackRow[]> {
    const page = await library.list({ mode: 'offset', offset: 0, limit, source, sort, direction });
    return page.items;
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = '';
    try {
      const [frequentRows, recentRows, addedRows, stats] = await Promise.all([
        loadSection({ kind: 'mostPlayed' }, FREQUENT_SIZE),
        loadSection({ kind: 'recent' }, FREQUENT_SIZE),
        loadSection({ kind: 'library' }, ADDED_SIZE, 'added', 'desc'),
        ipc.history.stats(),
      ]);
      frequent = frequentRows;
      recentPlayed = recentRows;
      recentAdded = addedRows;
      playStats = { totalPlays: stats.totalPlays, totalMs: stats.totalMs };
    } catch (error) {
      loadError = errorMessage(error);
    } finally {
      loading = false;
    }
  }

  /**
   * 曲库总数变化时重载（扫描完成、批量移出曲库都会改它）。
   * 不在 `onMount` 里自己发一次：`boot` 之后 `library.stats` 才就位，
   * 提前查会在"曲库还没准备好"的时刻白跑查询。
   */
  let lastTotal: number | null = null;
  $effect(() => {
    const total = library.stats?.tracks ?? null;
    if (total === null || total === lastTotal) return;
    lastTotal = total;
    void load();
  });

  /** 清空播放历史（常听/最近播放的输入都是播放历史，清空后两表同步变空） */
  async function clearHistory(): Promise<void> {
    await history.clear();
    void load();
  }

  async function play(rows: TrackRow[], index: number): Promise<void> {
    try {
      await player.playTracks(rows.map((row) => row.id), index);
    } catch (error) {
      // 起播失败不阻断运行：走播放条吐司（报错三档，2026-11 拍板）
      notify.showToast(errorMessage(error));
    }
  }

  /**
   * 封面即播放键：直接起播整个歌单，**不切视图**——仪表盘的"随手一点就有声"。
   * 查询用 cursor 模式（歌单顺序是语义，sort 字段被后端忽略），
   * id 收集由后端 `collect_track_ids` 兜底（不受窗口分页 limit 钳制）。
   */
  async function playPlaylist(id: string): Promise<void> {
    try {
      await player.playQuery(
        { mode: 'cursor', sort: 'title', direction: 'asc', source: { kind: 'playlist', playlistId: id } },
        0,
      );
    } catch (error) {
      notify.showToast(errorMessage(error));
    }
  }

  /** 统计卡展示值。library.stats / playStats 谁没到就显示 —，不各自转圈 */
  const tiles = $derived.by(() => {
    const s = library.stats;
    return [
      { label: '曲目', value: s ? String(s.tracks) : '—' },
      { label: '专辑', value: s ? String(s.albums) : '—' },
      { label: '累计播放', value: playStats ? formatPlays(playStats.totalPlays) : '—' },
      {
        label: '收听时长',
        value: playStats ? formatDurationLong(playStats.totalMs) : '—',
      },
    ];
  });
</script>

<div class="page-scroll home-page">
  {#if loadError}<InlineError message={loadError} ondismiss={() => (loadError = '')} />{/if}

  <!-- 第一行：歌单（约 70%）+ 统计小卡（约 30%） -->
  <div class="home-top">
    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <span class="page-kicker">PLAYLISTS</span>
          <h2>歌单</h2>
          <p class="section-desc">自己编排的收听合集</p>
        </div>
        <!-- 缺失警告的唯一的统计位：有缺失时顶掉计数（两者同占标题行右侧，
             点击去设置→曲库处理；性质是告警不是导航） -->
        {#if library.stats && library.stats.missingLocations > 0}
          <button class="stat-warn" onclick={() => nav.settingsAt('library')}>
            {library.stats.missingLocations} 个位置缺失，去处理
          </button>
        {:else}
            <span>共 {playlists.items.length} 个</span>
        {/if}
      </div>
      <div class="playlist-row">
        {#each playlists.items as playlist (playlist.id)}
          <div class="playlist-card">
            <button
              class="pl-cover"
              title={`播放「${playlist.name}」`}
              aria-label={`播放 ${playlist.name}`}
              disabled={playlist.trackCount === 0}
              onclick={() => void playPlaylist(playlist.id)}
            >
              {#if playlist.artworkId}
                <!-- 256 档：148px 卡在 150% 缩放下需要 ~222 物理像素，96 档会模糊 -->
                <img src={artworkUrl(playlist.artworkId, 256) ?? ''} alt="" loading="lazy" />
              {:else}
                <span class="playlist-card-icon"><ListMusic size={26} /></span>
              {/if}
              <span class="pl-play"><Play size={18} /></span>
            </button>
            <button class="pl-name" title={playlist.name} onclick={() => nav.go({ kind: 'playlist', id: playlist.id })}>
              {playlist.name}
            </button>
            <span class="pl-count">{playlist.trackCount} 首</span>
          </div>
        {:else}
          <EmptyState title="从曲目右键菜单或批量操作创建歌单。" />
        {/each}
      </div>
    </section>

    <!-- 统计卡：只陈述事实，不可点（导航归侧栏） -->
    <aside class="home-stats" aria-label="曲库与收听统计">
      <div class="stat-tiles">
        {#each tiles as tile (tile.label)}
          <div class="stat-tile">
            <span class="tile-value">{tile.value}</span>
            <span class="tile-label">{tile.label}</span>
          </div>
        {/each}
      </div>
    </aside>
  </div>

  <!-- 第二行：常听 | 最近播放。数据源同为播放历史，成对出现 -->
  <div class="home-duo">
    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <span class="page-kicker">MOST PLAYED</span>
          <h2>常听</h2>
          <p class="section-desc">播放次数排出来的曲目榜单</p>
        </div>
        <button class="section-link" onclick={() => void clearHistory()}>清空播放历史</button>
      </div>
      <TrackTable
        rows={frequent}
        columns={['cover', 'title', 'artist', 'plays']}
        {loading}
        empty="播放次数积累后，这里会长出你的常听曲目。"
        currentId={player.state?.current?.id ?? null}
        onplay={(index) => void play(frequent, index)}
        onopenartist={(id) => nav.go({ kind: 'artist', id })}
        onopenalbum={(id) => nav.go({ kind: 'album', id })}
      />
    </section>

    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <span class="page-kicker">RECENTLY PLAYED</span>
          <h2>最近播放</h2>
          <p class="section-desc">播放历史里最近的轨迹</p>
        </div>
      </div>
      <TrackTable
        rows={recentPlayed}
        columns={['cover', 'title', 'artist', 'lastPlayed']}
        {loading}
        empty="播放后，这里会留下最近的轨迹。"
        currentId={player.state?.current?.id ?? null}
        onplay={(index) => void play(recentPlayed, index)}
        onopenartist={(id) => nav.go({ kind: 'artist', id })}
        onopenalbum={(id) => nav.go({ kind: 'album', id })}
      />
    </section>
  </div>

  <!-- 第三行：最近添加。整页宽，多一列专辑 -->
  <section class="home-section">
    <div class="section-heading">
      <div class="section-title">
        <span class="page-kicker">RECENTLY ADDED</span>
        <h2>最近添加</h2>
        <p class="section-desc">最新入库的曲目</p>
      </div>
    </div>
    <TrackTable
      rows={recentAdded}
      columns={['cover', 'title', 'artist', 'album', 'added']}
      {loading}
      empty="扫描入库后，最新的曲目会出现在这里。"
      currentId={player.state?.current?.id ?? null}
      onplay={(index) => void play(recentAdded, index)}
      onopenartist={(id) => nav.go({ kind: 'artist', id })}
      onopenalbum={(id) => nav.go({ kind: 'album', id })}
    />
  </section>
</div>
