<script lang="ts">
  import {
    Activity,
    Disc,
    House,
    Library,
    Mic2,
    Plus,
    Radio,
    Settings,
    Tags,
  } from '@lucide/svelte';
  import { artworkUrl } from '../../lib/artwork';
  import { nav } from '../../stores/nav.svelte';
  import { overlay } from '../../stores/overlay.svelte';
  import { playlists } from '../../stores/playlists.svelte';
  import { ui } from '../../stores/ui.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';

  /**
   * 侧栏：**零 prop**，自己读 store、自己调 `nav.go`。
   *
   * 为什么零 prop（docs/24 §3.1、§5 B3a 的第 2 条）：侧栏、首页卡片、空态按钮、播放条
   * 的返回键**都是路由入口**，它们必须走同一个 `nav.go`（同一份历史栈）。
   * 用 prop 传回调时，AppShell 得给每一条入口写一个转发函数 —— 那次改名的教训是
   * "入口散在模板里，漏一个就变成点了没反应"。现在"我要去哪"直接说给 nav。
   *
   * ### 一级导航（2026-10-28 用户拍板重排）
   *
   * 上组：推荐 / 曲库 / 专辑 / 艺人 / FM 电台；底部：整理库 / 设置。
   * - **「正在播放」撤出侧栏**：唯一入口改在播放条左侧的歌曲区域
   *   ——"正在听什么"的信息本身就在那里，点它是比找侧栏更短的路。
   * - **「扫描入库 / 设置」沉到侧栏底部**（用户拍板）：内容导航和工具/
   *   配置是两类心智——听歌的东西在上，管理应用的东西在下（浏览器/
   *   IDE 的通行排法）。原底部统计卡片撤销，统计迁往首页卡片与扫描入库页。
   * - **「最近播放 / 常听单页」退役**（docs/28）：常听收进推荐页 top-15 段。
   * - **「扫描入库」这条线走完了**：docs/29 批 A 把它升成一级页（扫描任务是
   *   长时跑的，值得一个可停留可刷新的位置），docs/48 把 beets 拆成独立
   *   「整理库」页之后它只剩一张目录卡片，2026-10-30 并入设置页的「曲库」组，
   *   席位撤销。底部现在是「整理库 / 设置」两项。
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
  ] as const;

  /** 底部工具组：与内容导航分开渲染，但高亮/路由行为完全一致。 */
  const FOOTER_PAGES = [
    {
      key: 'beets',
      label: '整理库',
      icon: Tags,
      go: () => nav.go({ kind: 'page', page: 'beets' }),
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
  function isActive(key: (typeof PAGES | typeof FOOTER_PAGES)[number]['key']): boolean {
    if (key === 'albums') return ui.page === 'albums' || ui.page === 'album';
    if (key === 'artists') return ui.page === 'artists' || ui.page === 'artist';
    return ui.page === key;
  }

  function playlistActive(id: string): boolean {
    return ui.page === 'playlist' && ui.playlistId === id;
  }
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
        onclick={() => (overlay.dialog = { kind: 'playlistEdit' })}
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
          {#if playlist.kind === 'smart'}
            <span class="pl-badge" title="动态歌单：按规则收录曲目，「刷新」追加新命中">
              <Activity size={10} strokeWidth={2.4} aria-hidden="true" />
            </span>
          {/if}
          <span class="pl-count">{playlist.trackCount}</span>
        </button>
      {:else}
        <p class="side-empty">还没有歌单</p>
      {/each}
    </div>
  </div>

  <!-- 底部工具组（用户拍板）：原统计卡片撤销（统计迁首页卡片 + 设置页曲库组），
       席位让给应用管理两项——内容导航在上、应用管理在下的两类心智。
       2026-10-30：「扫描入库」席位撤销（那页并入设置页「曲库」组）。 -->
  <div class="sidebar-footer">
    {#each FOOTER_PAGES as item (item.key)}
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
    /* 半透明（T4c）：全应用背景从底下透出来。不用 backdrop-filter——
     * 持久壳层垫着动画频谱 canvas 就是每帧重模糊（docs/25 §5 红线）。
     * 右侧边线已撤（用户拍板"区域之间不留 border"）：与主区的分隔靠
     * 侧栏与主区底色的阶差。 */
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

  /* 动态歌单徽标：activity 图标（心电波形=随规则动的曲目），主题色。
     无边框——细边框在一行里太抢，靠颜色就够识别 */
  .pl-badge {
    flex: none;
    display: inline-flex;
    align-items: center;
    padding: 2px;
    color: var(--accent);
  }

  .pl-count {
    flex: none;
    font-size: 10px;
    line-height: 1;
    padding: 3px 6px;
    border-radius: 999px;
    background: var(--surface-hover); /* 半透明底：两个主题各自成立，不写死色值 */
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .side-empty {
    margin: 0;
    padding: 8px 10px;
    font-size: 11.5px;
    color: var(--text-3);
  }

  /* ---------------- 底部工具组 ---------------- */

  /* 原统计卡片席位。border-top 不再加（区域间不留 border 是本轮总规矩），
     与歌单区的分隔靠 margin；按钮复用 .nav-item 的皮，视觉与上组一致。 */
  .sidebar-footer {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 6px;
  }
</style>
