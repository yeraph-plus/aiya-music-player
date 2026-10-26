<script lang="ts">
  import { Disc, House, Library, Mic2, Plus, Radio, Settings } from '@lucide/svelte';
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
  * ### 一级导航的六项（docs/25 §3 重排 + docs/28 修订）

  * 推荐 / 曲库 / 专辑 / 艺人 / FM 电台 / 设置。
  * - **正在播放撤出侧栏**（用户拍板）：唯一入口改在播放条左侧的歌曲区域
  *   ——"正在听什么"的信息本身就在那里，点它是比找侧栏更短的路。
  * - **「首页」改名「推荐」**：歌单宫格 + 三段预览本来就是推荐位，名字跟上。
  * - **最近播放 / 常听单页退役**（docs/28）：常听收进推荐页 top-15 段，
  *   最近播放被筛选器取代；腾出的两个席位给**专辑 / 艺人宫格页**——
  *   曲库页 tab 撤掉后宫格需要一级入口，导航总数不变（仍是六项）。
  * - **FM 电台回归**：占位页也给出入口比死链接消失好——用户知道这功能在规划里。
  */

  const PAGES = [
    { key: 'home', label: '推荐', icon: House, go: () => nav.go({ kind: 'dashboard' }) },
    {
      key: 'library',
      label: '曲库',
      icon: Library,
      go: () => nav.go({ kind: 'builtin', view: 'library' }),
    },
    {
      key: 'albums',
      label: '专辑',
      icon: Disc,
      go: () => nav.go({ kind: 'browse', view: 'albums' }),
    },
    {
      key: 'artists',
      label: '艺人',
      icon: Mic2,
      go: () => nav.go({ kind: 'browse', view: 'artists' }),
    },
    {
      key: 'radio',
      label: 'FM 电台',
      icon: Radio,
      go: () => nav.go({ kind: 'page', page: 'radio' }),
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
   * B4-1 拆页后一项一个路由值，直接比 `page` 就行。宫格页与其详情页
   * **共用一个侧栏席位**（docs/28）：专辑页/专辑详情页都高亮「专辑」——
   * 详情页是从宫格或列表链接点进来的，高亮跟着"内容类别"走而不是
   * "路由值"，否则详情页上侧栏零高亮（e2e「高亮恰好一个」会抓住）。
   */
  function isActive(key: (typeof PAGES)[number]['key']): boolean {
    if (key === 'albums') return ui.page === 'albums' || ui.page === 'album';
    if (key === 'artists') return ui.page === 'artists' || ui.page === 'artist';
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
    /* 半透明（T4c）：全应用背景从底下透出来。不用 backdrop-filter——
     * 持久壳层垫着动画频谱 canvas 就是每帧重模糊（docs/25 §5 红线）。 */
    background: color-mix(in srgb, var(--surface-2) 82%, transparent);
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
