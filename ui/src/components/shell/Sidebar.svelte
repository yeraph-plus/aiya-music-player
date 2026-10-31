<script lang="ts">
  import {
    Activity,
    Disc,
    Disc3,
    House,
    Library,
    Mic2,
    Plus,
    Radio,
    Settings,
  } from '@lucide/svelte';
  import { artworkUrl } from '../../lib/artwork';
  import { nav } from '../../stores/nav.svelte';
  import { overlay } from '../../stores/overlay.svelte';
  import { playlists } from '../../stores/playlists.svelte';
  import { ui } from '../../stores/ui.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';

  /**
   * 侧栏：零 prop，自行读取 store 并调用 `nav.go`。
   *
   * 不接收 prop 的原因（docs/24 §3.1）：侧栏、首页卡片、空态按钮、播放条返回键
   * 都是路由入口，必须共用同一个 `nav.go` 与同一份历史栈。以 prop 传递回调时，
   * AppShell 需为每个入口编写转发函数，新增入口容易遗漏，表现为点击无响应。
   *
   * 一级导航分两组。上组：推荐、曲库、专辑、艺人、直播；底部：Beets、设置。
   *
   * - 「正在播放」不在侧栏：唯一入口为播放条左侧的曲目区域，该区域本身即展示
   *   当前播放内容，比经由侧栏更直接。
   * - 「设置」位于底部（docs/24 §3.1）：内容导航与应用配置分属两类，听歌相关在
   *   上、管理相关在下，与浏览器、IDE 的通行排法一致。
   * - 「最近播放 / 常听」无独立页面（docs/28）：常听收在推荐页 top-15 段。
   * - 「扫描入库」无独立席位：beets 独立成「Beets」页（docs/48）后，该页只剩
   *   一张曲库目录卡片，归入设置页的「曲库」组。
   * - 路由值为 `live`（docs/52 §7.4）：直播页接收 m3u8 地址播放。旧会话落在
   *   `radio` 上的由 `restore()` 的退役表换算；保留两个键会使"哪个有效"需查证。
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
      key: 'live',
      label: '直播',
      icon: Radio,
      go: () => nav.go({ kind: 'page', page: 'live' }),
    },
  ] as const;

  /** 底部工具组：与内容导航分开渲染，但高亮/路由行为完全一致。 */
  const FOOTER_PAGES = [
    {
      key: 'beets',
      label: 'Beets',
      icon: Disc3,
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
   * 一项一个路由值，直接比 `page`。宫格页与其详情页共用一个侧栏席位
   * （docs/28）：专辑页与专辑详情页都高亮「专辑」。详情页由宫格或列表链接进入，
   * 高亮跟"内容类别"走而不是"路由值"，否则详情页上侧栏零高亮
   * （e2e「高亮恰好一个」会抓住）。
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

  <!-- 底部工具组：容纳应用管理两项（Beets / 设置），与上方内容导航分区。
       统计不移入侧栏，而是分布在首页卡片与设置页「曲库」组。 -->
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
  /* `.sidebar` 的网格席位（grid-row / grid-column）定义在 layout.css，属四区布局；
     本文件只负责其外观。 */
  .sidebar {
    padding: 8px 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    /* 半透明：应用级背景从下层透出。不使用 backdrop-filter——持久壳层覆盖在
     * 动画频谱 canvas 之上时，每帧都需重新模糊（docs/25 §5）。
     * 不设右侧边线：与主区的分隔依靠底色阶差。 */
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
    font-size: var(--fs-sm);
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
    font-size: var(--fs-base);
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
    font-size: var(--fs-2xs);
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
    font-size: var(--fs-sm);
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
