<script lang="ts">
  import { AudioLines, Clock, Flame, House, Library, ListMusic, Plus, Settings } from '@lucide/svelte';
  import { artworkUrl } from '../../lib/artwork';
  import { shortenPath } from '../../lib/format';
  import { library } from '../../stores/library.svelte';
  import { nav } from '../../stores/nav.svelte';
  import { overlay } from '../../stores/overlay.svelte';
  import { playlists } from '../../stores/playlists.svelte';
  import { ui } from '../../stores/ui.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import StatBlock from '../ui/StatBlock.svelte';

  /**
   * 侧栏：**零 prop**，自己读 store、自己调 `nav.go`。
   *
   * 为什么零 prop（docs/24 §3.1、§5 B3a 的第 2 条）：侧栏、首页卡片、空态按钮、播放条
   * 的返回键**都是路由入口**，它们必须走同一个 `nav.go`（同一份历史栈）。
   * 用 prop 传回调时，AppShell 得给每一条入口写一个转发函数 —— 那次改名的教训是
   * "入口散在模板里，漏一个就变成点了没反应"。现在"我要去哪"直接说给 nav。
   *
   * ### 一级导航的六项
   *
   * 首页 / 曲库 / 最近播放 / 常听 / 正在播放 / 设置。
   * - **曲库三视图直接进侧栏**：以前要先回首页、再点卡片才能到"最近播放"，
   *   而这三者是曲库的**主要浏览方式**，不是首页的预览延伸。
   * - **FM 电台移出**：`RadioView` 是 10 行占位，占着一级导航等于给用户一个死入口（docs/24 §2 P10）。
   *   路由与组件暂留（`AppPage` 里还有 `'radio'`），等 B4 决定是删还是做。
   * - **专辑 / 艺人不在这一批**：它们要的是 group-by 查询（现在 `TrackQuery` 只有单值
   *   `artistId`/`albumId` 筛选），做入口等于做 B4 的数据层。
   */

  const PAGES = [
    { key: 'home', label: '首页', icon: House, go: () => nav.go({ kind: 'dashboard' }) },
    {
      key: 'library',
      label: '曲库',
      icon: Library,
      go: () => nav.go({ kind: 'builtin', view: 'library' }),
    },
    {
      key: 'recent',
      label: '最近播放',
      icon: Clock,
      go: () => nav.go({ kind: 'builtin', view: 'recent' }),
    },
    {
      key: 'mostPlayed',
      label: '常听',
      icon: Flame,
      go: () => nav.go({ kind: 'builtin', view: 'mostPlayed' }),
    },
    {
      key: 'nowPlaying',
      label: '正在播放',
      icon: AudioLines,
      go: () => nav.go({ kind: 'page', page: 'nowPlaying' }),
    },
    {
      key: 'settings',
      label: '设置',
      icon: Settings,
      go: () => nav.go({ kind: 'page', page: 'settings' }),
    },
  ] as const;

  /**
   * 判定"当前在哪"。
   *
   * B4-1 拆页后一项一个路由值，直接比 `page` 就行 —— 拆页前这里要写
   * `page === 'home' && libraryDetail && view.kind === key`，因为四个列表视图
   * 全挤在 `home` 上。判断从"三个字段的组合"退化成一个相等比较，是拆页
   * 顺带赚到的可读性。
   */
  function isActive(key: (typeof PAGES)[number]['key']): boolean {
    return ui.page === key;
  }

  function playlistActive(id: string): boolean {
    return ui.page === 'playlist' && ui.playlistId === id;
  }

  const stats = $derived(library.stats);
</script>

<aside class="sidebar">
  <nav class="nav" aria-label="主导航">
    {#each PAGES as item (item.key)}
      <button
        class="nav-item"
        class:active={isActive(item.key)}
        aria-current={isActive(item.key) ? 'page' : undefined}
        onclick={item.go}
      >
        <item.icon size={16} strokeWidth={1.8} />
        <span>{item.label}</span>
      </button>
    {/each}
  </nav>

  <div class="side-section">
    <div class="side-head">
      <span>歌单</span>
      <IconButton
        label="新建歌单"
        size="sm"
        onclick={() => (overlay.dialog = { kind: 'createPlaylist' })}
      >
        <Plus size={14} />
      </IconButton>
    </div>

    <div class="playlist-list">
      {#each playlists.items as playlist (playlist.id)}
        <button
          class="nav-item playlist-item"
          class:active={playlistActive(playlist.id)}
          aria-current={playlistActive(playlist.id) ? 'page' : undefined}
          title={`${playlist.name} · ${playlist.trackCount} 首`}
          onclick={() => nav.go({ kind: 'playlist', id: playlist.id })}
          oncontextmenu={(event) => {
            event.preventDefault();
            overlay.openPlaylistMenu(event.clientX, event.clientY, playlist);
          }}
        >
          <Cover
            src={playlist.artworkId ? artworkUrl(playlist.artworkId, 96) : null}
            size="xs"
            radius="sm"
          />
          <span class="pl-name">{playlist.name}</span>
          <span class="pl-count">{playlist.trackCount}</span>
        </button>
      {:else}
        <p class="side-empty">还没有歌单</p>
      {/each}
    </div>
  </div>

  <div class="sidebar-footer">
    {#if stats}
      <!-- 三个数字并排：tabular 数字保证滚动更新时不抖（StatBlock 自带） -->
      <div class="stat-row">
        <StatBlock size="sm" value={stats.tracks} label="曲目" />
        <StatBlock size="sm" value={stats.albums} label="专辑" />
        <StatBlock size="sm" value={stats.artists} label="艺人" />
      </div>
      {#if stats.missingLocations > 0}
        <span class="warn">{stats.missingLocations} 个位置缺失</span>
      {/if}
    {:else}
      <span class="dim">尚未建立曲库</span>
    {/if}
    {#if library.dataDir}
      <span class="dim path" title={library.dataDir}>{shortenPath(library.dataDir, 2)}</span>
    {/if}
  </div>
</aside>

<style>
  /* `.sidebar` 的网格席位（grid-row/grid-column）留在 layout.css —— 那是四区的事；
     这里只管它自己的皮。 */
  .sidebar {
    padding: 8px 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    border-right: 1px solid var(--line);
    background: var(--surface-2);
    backdrop-filter: blur(18px);
  }

  /* ---------------- 一级导航 ---------------- */

  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 30px;
    padding: 7px 9px;
    border-radius: 8px;
    color: var(--text-2);
    text-align: left;
  }

  .nav-item span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .nav-item:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .nav-item.active {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  /* 当前项的图标上色：文字已经是 text-1，再让图标 accent 一下才看得出"在哪"
     （只靠底色的话，深色主题下几乎看不出来） */
  .nav-item.active :global(svg) {
    color: var(--accent);
  }

  /* ---------------- 歌单分组 ---------------- */

  .side-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin-top: 10px;
  }

  .side-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    /* 10px 左内距对齐导航文字缩进；右 2px 让 + 号贴边但不顶到滚动条 */
    padding: 2px 2px 2px 9px;
    font-size: 11px;
    letter-spacing: 0.06em;
    color: var(--text-3);
    text-transform: uppercase;
  }

  .playlist-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-bottom: 6px;
  }

  .playlist-item {
    gap: 8px;
    /* 行内要放 22px 封面，纵向 padding 比导航项小 */
    padding: 4px 8px;
    min-height: 0;
  }

  .pl-name {
    flex: 1;
    font-size: 12px;
  }

  .pl-count {
    flex: none;
    font-size: 10.5px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .side-empty {
    margin: 0;
    padding: 8px 10px;
    font-size: 11.5px;
    color: var(--text-3);
  }

  /* ---------------- 底部统计 ---------------- */

  .sidebar-footer {
    flex: none;
    margin-top: auto;
    padding: 10px 2px 0;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 11px;
    line-height: 1.6;
    word-break: break-all;
  }

  .stat-row {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  .warn {
    color: var(--warn);
  }

  .dim {
    color: var(--text-2);
  }

  .path {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
