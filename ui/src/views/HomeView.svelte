<script lang="ts">
  import { Flame, ListMusic, Play } from '@lucide/svelte';
  import InlineError from '../components/ui/InlineError.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import TrackTable from '../components/TrackTable.svelte';
  import type { TrackRow } from '../lib/ipc';
  import type { TrackSource } from '../lib/query';
  import { artworkUrl } from '../lib/artwork';
  import { errorMessage } from '../lib/errors';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { playlists } from '../stores/playlists.svelte';

  /**
   * 推荐页（原「首页」）——**仪表盘**（2026-10-28 用户拍板重排）：
   *
   * - 第一级 歌单卡片行：封面即播放键（点击直接起播，不跳页），
   *   卡片名点击才进歌单视图；
   * - 第二级 常听榜：播放最多的 15 首（排序 = 次数 desc，同次数按最近优先），
   *   第 16 名永远进不来，满额后新歌挤掉末位——天然"新增一个退出一个"。
   *
   * 2026-10-28 二次修订：三项统计入口撤销（视觉冗余——曲目/专辑/艺人
   * 侧栏都有一级导航）。只剩缺失警告这一个功能性提醒。
   *
   * 标题层级（Header）与「全部曲目」路由卡撤销：仪表盘没有页标题，
   * 侧栏已经是全局导航。「最近播放 / 最近添加」两段迁往曲库页 tab
   * （list store 的 libraryTab），不再在这里占两屏。
   */
  const FREQUENT_SIZE = 15;

  let frequent = $state<TrackRow[]>([]);
  let loading = $state(true);
  let loadError = $state('');

  async function loadSection(source: TrackSource, limit: number): Promise<TrackRow[]> {
    const page = await library.list({ mode: 'offset', offset: 0, limit, source });
    return page.items;
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = '';
    try {
      frequent = await loadSection({ kind: 'mostPlayed' }, FREQUENT_SIZE);
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

  /** 清空播放历史（常听榜的输入就是播放计数，清空后榜单同步变空） */
  async function clearHistory(): Promise<void> {
    await history.clear();
    void load();
  }

  async function play(rows: TrackRow[], index: number): Promise<void> {
    try {
      await player.playTracks(rows.map((row) => row.id), index);
    } catch (error) {
      notify.showError(errorMessage(error));
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
      notify.showError(errorMessage(error));
    }
  }
</script>

<div class="page-scroll home-page">
  {#if loadError}<InlineError message={loadError} ondismiss={() => (loadError = '')} />{/if}

  <!-- 第一级：歌单卡片行。封面 = 播放键，名字 = 路由入口 -->
  <section class="home-section">
    <div class="section-heading">
      <div><ListMusic size={17} /><h2>歌单</h2></div>
      <span>{playlists.items.length} 个</span>
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
              <img src={artworkUrl(playlist.artworkId, 320) ?? ''} alt="" loading="lazy" />
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

  <!-- 缺失警告（2026-10-28：三项统计入口撤销后唯一保留的统计位——
       它不是导航是告警，点了去扫描入库页处理） -->
  {#if library.stats && library.stats.missingLocations > 0}
    <button class="stat-warn" onclick={() => nav.go({ kind: 'page', page: 'tools' })}>
      {library.stats.missingLocations} 个位置缺失，去「扫描入库」查看
    </button>
  {/if}

  <!-- 第二级：常听榜。清空历史的唯一入口挂在这里（榜单的输入就是播放计数） -->
  <section class="home-section">
    <div class="section-heading">
      <div><Flame size={17} /><h2>常听榜</h2></div>
      <button class="section-link" onclick={() => void clearHistory()}>清空播放历史</button>
    </div>
    <TrackTable
      rows={frequent}
      {loading}
      empty="播放次数积累后，这里会形成你的常听榜单。"
      valueKind="plays"
      currentId={player.state?.current?.id ?? null}
      onplay={(index) => void play(frequent, index)}
      onopenartist={(id) => nav.go({ kind: 'artist', id })}
      onopenalbum={(id) => nav.go({ kind: 'album', id })}
    />
  </section>
</div>
