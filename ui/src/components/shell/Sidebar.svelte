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
  import { t } from '../../lib/i18n.svelte';
  import { nav } from '../../stores/nav.svelte';
  import { overlay } from '../../stores/overlay.svelte';
  import { playlists } from '../../stores/playlists.svelte';
  import { ui } from '../../stores/ui.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';

  /**
   * 侧栏：零 prop，自行读取 store 并调用 `nav.go`。
   *
   * 不接收 prop 的原因：侧栏、首页卡片、空态按钮、播放条返回键
   * 都是路由入口，必须共用同一个 `nav.go` 与同一份历史栈。以 prop 传递回调时，
   * AppShell 需为每个入口编写转发函数，新增入口容易遗漏，表现为点击无响应。
   *
   * 一级导航分两组。上组：推荐、曲库、专辑、艺人、直播；底部：Beets、设置。
   *
   * - 「正在播放」不在侧栏：唯一入口为播放条左侧的曲目区域，该区域本身即展示
   *   当前播放内容，比经由侧栏更直接。
   * 「设置」位于底部：内容导航与应用配置分属两类，听歌相关在
   *   上、管理相关在下，与浏览器、IDE 的通行排法一致。
   * 「最近播放 / 常听」无独立页面：常听收在推荐页 top-15 段。
   * 「扫描入库」无独立席位：beets 独立成「Beets」页后，该页只剩
   *   一张曲库目录卡片，归入设置页的「曲库」组。
   * 路由值为 `live`：直播页接收 m3u8 地址播放。旧会话落在
   *   `radio` 上的由 `restore()` 的退役表换算；保留两个键会使"哪个有效"需查证。
   */

  const PAGES = [
    { key: 'home', labelKey: 'shell.nav.home', icon: House, go: () => nav.go({ kind: 'dashboard' }) },
    {
      key: 'library',
      labelKey: 'shell.nav.library',
      icon: Library,
      go: () => nav.go({ kind: 'builtin', view: 'library' }),
    },
    {
      key: 'albums',
      labelKey: 'shell.nav.albums',
      icon: Disc,
      go: () => nav.go({ kind: 'browse', view: 'albums' }),
    },
    {
      key: 'artists',
      labelKey: 'shell.nav.artists',
      icon: Mic2,
      go: () => nav.go({ kind: 'browse', view: 'artists' }),
    },
    {
      key: 'live',
      labelKey: 'shell.nav.live',
      icon: Radio,
      go: () => nav.go({ kind: 'page', page: 'live' }),
    },
  ] as const;

  /** 底部工具组：与内容导航分开渲染，但高亮/路由行为完全一致。 */
  const FOOTER_PAGES = [
    {
      key: 'beets',
      labelKey: 'shell.nav.beets',
      icon: Disc3,
      go: () => nav.go({ kind: 'page', page: 'beets' }),
    },
    {
      key: 'settings',
      labelKey: 'shell.nav.settings',
      icon: Settings,
      go: () => nav.go({ kind: 'page', page: 'settings' }),
    },
  ] as const;

  /**
   * 判定"当前在哪"。
   *
   * 一项一个路由值，直接比 `page`。宫格页与其详情页共用一个侧栏席位
   * 专辑页与专辑详情页都高亮「专辑」。详情页由宫格或列表链接进入
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

  /* 侧栏歌单拖动排序（HTML5 DnD）。排序真源是 `sort_position`，落点走
     `playlists.reorderTo`；键盘路径是右键菜单的「上移/下移」。
     自定义 MIME 类型：文件拖入（drop.rs 那条链）与这里的内部拖动互不误认。 */
  const DRAG_TYPE = 'application/x-aiya-playlist';
  /** 被拖项在当前顺序里的下标；-1 = 没有进行中的拖动 */
  let dragFrom = $state(-1);
  /** 插入位（原坐标 0..n，第 i 项上半 = i、下半 = i+1）；-1 = 原位，不亮指示 */
  let dropAt = $state(-1);

  function onItemDragStart(event: DragEvent, index: number): void {
    dragFrom = index;
    dropAt = -1;
    const dt = event.dataTransfer;
    if (!dt) return;
    dt.effectAllowed = 'move';
    // setData 必须有：没有数据的 dragstart 在 Chromium 里干脆不启动拖拽
    dt.setData(DRAG_TYPE, String(index));
  }

  function onItemDragOver(event: DragEvent, index: number): void {
    const dt = event.dataTransfer;
    if (dragFrom < 0 || !dt || !dt.types.includes(DRAG_TYPE)) return;
    event.preventDefault();
    dt.dropEffect = 'move';
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const at = index + (event.clientY < rect.top + rect.height / 2 ? 0 : 1);
    // 原位（自己/自己后面一格）不亮指示，落下去也是空操作
    dropAt = at === dragFrom || at === dragFrom + 1 ? -1 : at;
  }

  /** 列表级 dragover：拖到容器上下边缘时步进滚动——20 个歌单滚出屏后，
   *  没有它就拖不进屏幕外的位置 */
  function onListDragOver(event: DragEvent): void {
    if (dragFrom < 0) return;
    const list = event.currentTarget as HTMLElement;
    const rect = list.getBoundingClientRect();
    if (event.clientY < rect.top + 28) list.scrollTop -= 10;
    else if (event.clientY > rect.bottom - 28) list.scrollTop += 10;
  }

  function onListDrop(event: DragEvent): void {
    if (dragFrom < 0) return;
    event.preventDefault();
    const from = dragFrom;
    const at = dropAt;
    resetDrag();
    if (at < 0) return;
    // 后端 remove(from)+insert(to)：插入位在原坐标 from 之后时要减一
    void playlists.reorderTo(from, at > from ? at - 1 : at);
  }

  function resetDrag(): void {
    dragFrom = -1;
    dropAt = -1;
  }
</script>

<aside class="sidebar">
  <nav class="nav" aria-label={t('shell.sidebar.nav')}>
    {#each PAGES as item (item.key)}
      <button
        class="nav-item"
        class:active={isActive(item.key)}
        aria-current={isActive(item.key) ? 'page' : undefined}
        onclick={item.go}
      >
        <item.icon size={16} strokeWidth={1.8} />
        <span>{t(item.labelKey)}</span>
      </button>
    {/each}
  </nav>

  <div class="side-section">
    <div class="side-head">
      <span>{t('shell.sidebar.playlists')}</span>
      <IconButton
        label={t('shell.sidebar.newPlaylist')}
        size="sm"
        onclick={() => (overlay.dialog = { kind: 'playlistEdit' })}
      >
        <Plus size={14} />
      </IconButton>
    </div>

    <div
      class="playlist-list"
      role="group"
      ondragover={onListDragOver}
      ondrop={onListDrop}
    >
      {#each playlists.items as playlist, i (playlist.id)}
        <button
          class="nav-item playlist-item"
          class:active={playlistActive(playlist.id)}
          class:dragging={dragFrom === i}
          class:drop-above={dropAt === i}
          class:drop-below={dropAt === i + 1}
          aria-current={playlistActive(playlist.id) ? 'page' : undefined}
          title={t('shell.sidebar.playlistTip', { name: playlist.name, n: playlist.trackCount })}
          draggable="true"
          onclick={() => nav.go({ kind: 'playlist', id: playlist.id })}
          ondragstart={(event) => onItemDragStart(event, i)}
          ondragover={(event) => onItemDragOver(event, i)}
          ondragend={resetDrag}
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
            <span class="pl-badge" title={t('shell.sidebar.smartBadgeTip')}>
              <Activity size={10} strokeWidth={2.4} aria-hidden="true" />
            </span>
          {/if}
          <span class="pl-count">{playlist.trackCount}</span>
        </button>
      {:else}
        <p class="side-empty">{t('shell.sidebar.empty')}</p>
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
        <span>{t(item.labelKey)}</span>
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
     * 动画频谱 canvas 之上时，每帧都需重新模糊。
     * 不设右侧边线：与主区的分隔依靠底色阶差。 */
    background: color-mix(in srgb, var(--surface-2) 82%, transparent);
  }

  /* 一级导航 */

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

  /* 歌单分组 */

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
    /* 歌单列表是纵滚容器，子项必须有固定高度才能"溢出→滚动"：
       默认的 flex-shrink:1 会把所有行压扁塞进视口（歌单一多每行挤成
       一条细线，滚动永远不出现） */
    flex-shrink: 0;
  }

  /* 拖动排序的视觉：被拖项半透明（"提起"），落点用 accent 细线标在
   * 上/下边界。inset box-shadow 不占布局，行高在拖动全程不变 */
  .playlist-item.dragging {
    opacity: 0.45;
  }

  .playlist-item.drop-above {
    box-shadow: inset 0 2px 0 var(--accent);
  }

  .playlist-item.drop-below {
    box-shadow: inset 0 -2px 0 var(--accent);
  }

  .pl-name {
    flex: 1;
    font-size: var(--fs-base);
    /* 压回常规字重：pages.css 的 .pl-name（首页卡名档，600）是全局规则，
       会漏进这里把侧栏歌单名一起带粗——侧栏文本与导航项同档，用常规 */
    font-weight: 400;
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

  /* 底部工具组 */

  /* 原统计卡片席位。border-top 不再加（区域间不留 border 是本轮总规矩），
     与歌单区的分隔靠 margin；按钮复用 .nav-item 的皮，视觉与上组一致。 */
  .sidebar-footer {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 6px;
    /* 与上方歌单区分隔：Beets/设置是应用级入口，和内容导航不是一个族
       （线比空行可靠——歌单列表滚到底时内容贴着 footer，空行分不开） */
    border-top: 1px solid var(--line);
    padding-top: 8px;
  }
</style>
