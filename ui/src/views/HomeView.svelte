<script lang="ts">
  import { ListMusic, Play } from '@lucide/svelte';
  import TrackTable from '../components/TrackTable.svelte';
  import InlineError from '../components/ui/InlineError.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import { ipc } from '../lib/ipc';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSource } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { t, type Msg } from '../lib/i18n.svelte';
  import { errorMessage } from '../lib/errors';
  import { formatDurationLong, formatPlays } from '../lib/format';
  import { library } from '../stores/library.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';

  /**
   * 推荐页：仪表盘形态，三行布局。
   *
 * - 第一行 歌单行（横向滚动）+ 播放统计：封面即播放键（点击直接起播，不跳页），
 *   卡片名点击才进歌单视图；统计以时段问候打头，下面两组"标签 + 大字号
 *   数值"，顶部预留一个区块标题的高度与卡面顶沿齐平（pages.css
 *   `.home-stats::before` 占位），不与卡组几何互钉——卡组空态/加载态不影响它，
 *   **不可点**——一级导航在侧栏，统计是只读展示。
   * - 第二行 双表（常听 | 最近播放）：同一 TrackTable 组件、同列数，
   *   一个回答"你最常听什么"，一个回答"你刚听过什么"，数据源同为
   *   播放历史，放一行成对出现。
   * - 第三行 最近添加：整页宽单列表（多一列专辑），新入库曲目的出口。
   *
   * 区块标题只有一行：大字标题 + 小字英文装饰 +（歌单区一枚计数徽章）——
   * 一行式保持仪表盘密度，陈述行没有信息增量。
   *
   * 首页不放"清空播放历史"这类**破坏性**入口：仪表盘是浏览的地方，清理动作归
   * 设置与曲库页（两处都已具备）——只读面板不挂动作链接。
   *
 * 三个短列表区块的标题行是纯文字；区块的视觉边界落在表格表头的下横线上，
 * 行本体与主列表同构、直接浮在动态背景上。
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

  async function play(rows: TrackRow[], index: number): Promise<void> {
    try {
      await player.playTracks(rows.map((row) => row.id), index);
    } catch (error) {
      // 起播失败不阻断运行：走播放条吐司（报错三档）
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

  /**
   * 统计区只有两项：播放数与收听时长（曲目/专辑数撤掉——曲库规模在侧栏与曲库页
   * 都能看到，摆在首页统计位上是重复）。未加载到之前显示 `—`，不转圈：
   * 两行数字很快到，骨架反而闪。
   */
  const totalPlays = $derived(
    playStats && playStats.totalPlays > 0
      ? t('now.home.playsValue', { n: formatPlays(playStats.totalPlays) })
      : '—',
  );
  const totalSpan = $derived(playStats ? formatDurationLong(playStats.totalMs) : '—');

  /**
   * 时段问候，按系统小时分四段（5/11/13/18 为界）。组件装载时求值一次就够：
   * 问候跨段差几分钟没人会在意，为它挂一个时钟定时器不值得。
   * 整句进 i18n 表（英文语序与中文不同，不能只换单位）。
   */
  function greetingKey(hour: number): Msg {
    if (hour < 5) return 'home.greeting.night';
    if (hour < 11) return 'home.greeting.morning';
    if (hour < 13) return 'home.greeting.noon';
    if (hour < 18) return 'home.greeting.afternoon';
    return 'home.greeting.evening';
  }
  const greeting = $derived(t(greetingKey(new Date().getHours())));
</script>

<div class="page-scroll home-page">
  {#if loadError}<InlineError message={loadError} ondismiss={() => (loadError = '')} />{/if}

  <!-- 第一行：歌单（横向滚动）+ 播放统计 -->
  <div class="home-top">
    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <h2>{t('shell.sidebar.playlists')}</h2>
          <span class="page-kicker">PLAYLISTS</span>
          <span class="section-meta">{t('now.home.playlistCount', { n: playlists.items.length })}</span>
        </div>
        <!-- 缺失警告的唯一的统计位：有缺失时出现在标题行右侧，点击去设置→
             曲库处理；性质是告警不是导航 -->
        {#if library.stats && library.stats.missingLocations > 0}
          <button class="stat-warn" onclick={() => nav.settingsAt('library')}>
            {t('now.home.missingLocationsCta', { n: library.stats.missingLocations })}
          </button>
        {/if}
      </div>
      <div class="playlist-row">
        {#each playlists.items as playlist (playlist.id)}
          <div class="playlist-card">
            <button
              class="pl-cover"
              title={t('now.home.playPlaylistTip', { name: playlist.name })}
              aria-label={t('now.home.playPlaylistAria', { name: playlist.name })}
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
            <span class="pl-count">{t('common.trackCount', { n: playlist.trackCount })}</span>
          </div>
        {:else}
          <EmptyState title={t('now.home.playlistEmpty')} />
        {/each}
      </div>
    </section>

    <!-- 播放统计：时段问候 + 两组"标签 + 大字号数值"，顶部占位一个标题组件
         高度后与卡面顶沿齐平（不随卡组行高变）。不可点（导航归侧栏） -->
    <aside class="home-stats" aria-label={t('now.home.statsAria')}>
      <p class="stat-greet">{greeting}</p>
      <div class="stat-block">
        <span class="stat-label">{t('now.home.statTotalPlays')}</span>
        <span class="stat-value">{totalPlays}</span>
      </div>
      <div class="stat-block">
        <span class="stat-label">{t('now.home.statListeningTime')}</span>
        <span class="stat-value">{totalSpan}</span>
      </div>
    </aside>
  </div>

  <!-- 第二行：常听 | 最近播放。数据源同为播放历史，成对出现 -->
  <div class="home-duo">
    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <h2>{t('now.home.mostPlayed')}</h2>
          <span class="page-kicker">MOST PLAYED</span>
        </div>
      </div>
      <div class="home-panel">
        <TrackTable
          rows={frequent}
          columns={['cover', 'title', 'artist', 'plays']}
          {loading}
          empty={t('now.home.mostPlayedEmpty')}
          currentId={player.state?.current?.id ?? null}
          onplay={(index) => void play(frequent, index)}
          onopenartist={(id) => nav.go({ kind: 'artist', id })}
          onopenalbum={(id) => nav.go({ kind: 'album', id })}
        />
      </div>
    </section>

    <section class="home-section">
      <div class="section-heading">
        <div class="section-title">
          <h2>{t('library.tabRecent')}</h2>
          <span class="page-kicker">RECENTLY PLAYED</span>
        </div>
      </div>
      <div class="home-panel">
        <TrackTable
          rows={recentPlayed}
          columns={['cover', 'title', 'artist', 'lastPlayed']}
          {loading}
          empty={t('now.home.recentPlayedEmpty')}
          currentId={player.state?.current?.id ?? null}
          onplay={(index) => void play(recentPlayed, index)}
          onopenartist={(id) => nav.go({ kind: 'artist', id })}
          onopenalbum={(id) => nav.go({ kind: 'album', id })}
        />
      </div>
    </section>
  </div>

  <!-- 第三行：最近添加。整页宽，多一列专辑 -->
  <section class="home-section">
    <div class="section-heading">
      <div class="section-title">
        <h2>{t('library.tabAdded')}</h2>
        <span class="page-kicker">RECENTLY ADDED</span>
      </div>
    </div>
    <div class="home-panel">
      <TrackTable
        rows={recentAdded}
        columns={['cover', 'title', 'artist', 'album', 'added']}
        {loading}
        empty={t('now.home.recentAddedEmpty')}
        currentId={player.state?.current?.id ?? null}
        onplay={(index) => void play(recentAdded, index)}
        onopenartist={(id) => nav.go({ kind: 'artist', id })}
        onopenalbum={(id) => nav.go({ kind: 'album', id })}
      />
    </div>
  </section>
</div>
