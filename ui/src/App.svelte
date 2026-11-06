<script lang="ts">
  /**
   * 应用根：**装配 + 全局副作用**，没有业务逻辑。
   *
   * 批次 3 之前这里 1070 行（列表查询、歌单 CRUD、批量操作、快捷键、
   * 五个对话框全在里面）。现在：
   * - 版面 → `components/AppShell.svelte`（零 prop，自己读 store）
   * - 主区 → `views/LibraryView.svelte`
   * - 浮层 → `components/OverlayHost.svelte`
   * - 列表/歌单/扫描/历史编排 → `stores/*.svelte.ts`
   * - 快捷键 → `lib/keyboard.ts`
   * - 启动引导 → `lib/boot.ts`
   *
   * 留在这里的 4 个 `$effect` 有一个共同点：**它们操作的是 `document` 级
   * 全局状态**（`<html>` 上的主题属性、CSS 变量、媒体会话、歌词预取），
   * 既不属于某个视图，也不该塞进 store（store 不碰 DOM）。
   */
  import { onMount } from 'svelte';
  import { Tooltip } from 'bits-ui';
  import AppShell from './components/AppShell.svelte';
  import OverlayHost from './components/OverlayHost.svelte';
  import AlbumView from './views/AlbumView.svelte';
  import ArtistView from './views/ArtistView.svelte';
  import BrowseView from './views/BrowseView.svelte';
  import BeetsView from './views/BeetsView.svelte';
  import LibraryView from './views/LibraryView.svelte';
  import LiveView from './views/LiveView.svelte';
  import HomeView from './views/HomeView.svelte';
  import NowPlayingView from './views/NowPlayingView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import { boot } from './lib/boot';
  import { onKeydown } from './lib/keyboard';
  import { createMediaSession, type MediaSessionBridge } from './lib/media-session';
  import { applyLook, readRowHeight, watchSystemTheme, writeLook } from './lib/look';
  import { drop } from './stores/drop.svelte';
  import { list } from './stores/list.svelte';
  import { notify } from './stores/notify.svelte';
  import { setLocale } from './lib/i18n.svelte';
  import { player } from './stores/player.svelte';
  import { lyrics } from './stores/lyrics.svelte';
  import { isListPage, ui } from './stores/ui.svelte';

  let mediaSession: MediaSessionBridge | null = null;

  // 登录门：http transport 收到 401 时派发 `aiya:gate`（auth.rs 的登录门）。
  // 首帧值来自进闸预检（main.ts 在 mount 前跑 /auth/state，写入
  // `__AIYA_GATE__`）——门开启时登录卡首帧即渲染，主界面不闪一帧。
  // 事件驱动而非 store——lib 层不得反向依赖 stores（dep-graph 门禁口径）。
  let gate = $state(!!(window as { __AIYA_GATE__?: boolean }).__AIYA_GATE__);
  $effect(() => {
    const on = () => (gate = true);
    window.addEventListener('aiya:gate', on);
    return () => window.removeEventListener('aiya:gate', on);
  });

  /**
   * 系统媒体中心：只订阅 PlayerState，与音频出口无关（出声在 Rust 侧）。
   * SMTC 常开无开关（2026-11-04 拍板）：标题/封面同步与面板控制键全量注册，
   * 不再有"按键接管与否"的运行期变化，创建一次活到卸载。
   */
  function buildMediaSession(): MediaSessionBridge {
    return createMediaSession({
      toggle: () => player.toggle(),
      next: () => player.next(),
      prev: () => player.prev(),
      seek: (ms) => player.seek(ms),
      skip: (seconds) => player.seek((player.state?.positionMs ?? 0) + seconds * 1000),
      stop: () => player.clear(),
    });
  }

  // 主题/主题颜色/界面缩放：前两者写在 <html> 的 data 属性上，缩放写内联 font-size，CSS 变量随之切换，无需重建组件。同时写一份
  // localStorage 镜像，供下次启动首帧使用（否则浅色主题会先闪一帧深色）。
  $effect(() => {
    const look = {
      theme: ui.settings.theme,
      ambient: ui.settings.ambient,
      uiScale: ui.settings.uiScale,
    };
    applyLook(look);
    writeLook(look);
    // 行高只有 CSS 一处定义，这里把它的当前值同步给虚拟列表。
    // 缩放档变化也会走到这里：--row-h 是 rem，行高跟着档位变，列表要重新对齐。
    ui.rowHeight = readRowHeight();

    // 跟随系统档：操作系统切换深浅色时重放主题属性（data-theme 取解析值，
    // 值变了 accent 观察器、频谱取色随之刷新）。行高不随主题变，无需重算。
    // 仅跟随档订阅；固定深/浅档的系统事件无意义。
    if (look.theme !== 'system') return;
    return watchSystemTheme(() => applyLook(look));
  });

  // 界面语言：设置值驱动 i18n locale 与 <html data-lang>（装饰字取消等纯 CSS
  // 分支靠它）。首启语言由宿主按系统语言写库，前端只跟随；
  // 空值（mock 预览）按 zh 兜底。t() 读 i18n.svelte.ts 的 $state，模板自动重渲染。
  $effect(() => {
    const lang = ui.settings.language === 'en' ? 'en' : 'zh';
    setLocale(lang);
    document.documentElement.dataset.lang = lang;
  });

  // 正在播放页打开且换歌时重新取词；离开页面不预取，省一次读盘。
  $effect(() => {
    const trackId = player.state?.current?.id ?? null;
    if (ui.page !== 'nowPlaying' || !trackId) return;
    lyrics.load(trackId);
  });

  onMount(() => {
    // 登录闸打开时跳过整条启动链（审查 P2 降噪）：过期会话不再在登录卡背后
    // 打 1Hz 的 401 回报与 SSE 心跳。登录成功 = 整页重载，mount 自然重走
    if (gate) return;
    mediaSession = buildMediaSession();

    const disposeBoot = boot({ onState: (next) => mediaSession?.update(next) });
    // 壳级拖放：整窗都是落点，不分页面
    const disposeDrop = drop.attach();

    return () => {
      disposeBoot();
      disposeDrop();
      notify.dispose();
      list.dispose();
      ui.dispose();
      mediaSession?.dispose();
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<!-- `Tooltip.Provider` **只放这一处**：它统一 `delayDuration`，并让相邻 tooltip
之间共享"刚从别的按钮移过来就不用再等"的计时。每个 Tooltip 自带一份的话，鼠标横扫一排图标每次都要等满延迟。 -->
{#if gate}
  <!-- 动态分包：登录门只有 http 传输会进（401 → `aiya:gate`），桌面构建里该
  chunk 永不加载——"桌面没有令牌门禁实现"由装载路径钉死，与 http 传输 chunk
  同机制。 -->
  {#await import('./components/LoginView.svelte') then { default: LoginView }}
    <LoginView />
  {/await}
{:else}
<Tooltip.Provider delayDuration={400} skipDelayDuration={300}>
  <AppShell>
    <!--
      一名一路由。拆页（B4-1）之前这里是 `page === 'home'` 外面再套一层
      `libraryDetail`，六个列表视图全靠一个 boolean 区分；现在列表页
      共用同一套列表查询，路由值直接决定渲染谁。
      专辑/艺人详情页有独立视图（含头部聚合数据），先于
      LibraryView 判断。
    -->
    {#if ui.page === 'home'}
      <HomeView />
    {:else if ui.page === 'albums' || ui.page === 'artists'}
      <BrowseView />
    {:else if ui.page === 'album'}
      <AlbumView />
    {:else if ui.page === 'artist'}
      <ArtistView />
    {:else if isListPage(ui.page)}
      <LibraryView />
    {:else if ui.page === 'nowPlaying'}
      <NowPlayingView />
    {:else if ui.page === 'live'}
      <LiveView />
    {:else if ui.page === 'beets'}
      <BeetsView />
    {:else}
      <SettingsView />
    {/if}
  </AppShell>

  <OverlayHost />
</Tooltip.Provider>
{/if}
