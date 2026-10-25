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
  import AppShell from './components/AppShell.svelte';
  import OverlayHost from './components/OverlayHost.svelte';
  import LibraryView from './views/LibraryView.svelte';
  import HomeView from './views/HomeView.svelte';
  import NowPlayingView from './views/NowPlayingView.svelte';
  import RadioView from './views/RadioView.svelte';
  import SettingsView from './views/SettingsView.svelte';
  import { accentForArtwork } from './lib/accent';
  import { artworkUrl } from './lib/artwork';
  import { boot } from './lib/boot';
  import { onKeydown } from './lib/keyboard';
  import { createMediaSession, type MediaSessionBridge } from './lib/media-session';
  import { applyLook, writeLook } from './lib/look';
  import { list } from './stores/list.svelte';
  import { notify } from './stores/notify.svelte';
  import { player } from './stores/player.svelte';
  import { ui } from './stores/ui.svelte';

  let mediaSession: MediaSessionBridge | null = null;

  /**
   * 系统媒体中心：只订阅 PlayerState，与音频出口无关（出声在 Rust 侧）。
   * `actions` 决定是否接管系统媒体键——开关变化时必须**重建**，
   * 因为处理器只能在创建时注册。
   */
  function buildMediaSession(): MediaSessionBridge {
    return createMediaSession(
      {
        toggle: () => player.toggle(),
        next: () => player.next(),
        prev: () => player.prev(),
        seek: (ms) => player.seek(ms),
        skip: (seconds) => player.seek((player.state?.positionMs ?? 0) + seconds * 1000),
        stop: () => player.clear(),
      },
      { actions: ui.settings.mediaKeys },
    );
  }

  // 主题与密度：写在 <html> 的 data 属性上，CSS 变量随之切换，无需重建组件。
  // 同时写一份 localStorage 镜像，供下次启动首帧使用（否则浅色主题会先闪一帧深色）。
  $effect(() => {
    const look = { theme: ui.settings.theme, density: ui.settings.density };
    applyLook(look);
    writeLook(look);
  });

  // 封面主色：切歌或开关变化时重新提取；异步结果带取消标记，防止旧封面覆盖新颜色
  $effect(() => {
    const artworkId = player.state?.current?.artworkId ?? null;
    if (!ui.settings.accentFromArtwork || !artworkId) {
      document.documentElement.style.removeProperty('--accent');
      document.documentElement.style.removeProperty('--accent-soft');
      return;
    }

    let cancelled = false;
    const url = artworkUrl(artworkId, 96);
    if (!url) return;
    void accentForArtwork(url).then((hex) => {
      if (cancelled || !hex) return;
      document.documentElement.style.setProperty('--accent', hex);
      document.documentElement.style.setProperty('--accent-soft', `${hex}33`);
    });
    return () => {
      cancelled = true;
    };
  });

  /**
   * 系统媒体键开关。（ReplayGain 归一化**不在这里**：增益在 Rust DSP 链内
   * 应用，开关由 `app_set_settings` 的设置标志驱动。）
   *
   * `mediaSessionActions` 记录当前生效值，避免设置对象每次变化都重建
   * （比如只改主题）。
   */
  let mediaSessionActions: boolean | null = null;
  $effect(() => {
    const enabled = ui.settings.mediaKeys;
    if (mediaSessionActions === null || mediaSessionActions === enabled) return;
    mediaSessionActions = enabled;
    mediaSession?.dispose();
    mediaSession = buildMediaSession();
    mediaSession.update(player.state);
  });

  // 正在播放页打开且换歌时重新取词；离开页面不预取，省一次读盘。
  $effect(() => {
    const trackId = player.state?.current?.id ?? null;
    if (ui.page !== 'nowPlaying' || !trackId) return;
    ui.loadLyrics(trackId);
  });

  onMount(() => {
    mediaSession = buildMediaSession();
    mediaSessionActions = ui.settings.mediaKeys;

    const disposeBoot = boot({ onState: (next) => mediaSession?.update(next) });

    return () => {
      disposeBoot();
      notify.dispose();
      list.dispose();
      ui.dispose();
      mediaSession?.dispose();
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<AppShell>
  {#if ui.page === 'home'}
    {#if ui.libraryDetail}<LibraryView />{:else}<HomeView />{/if}
  {:else if ui.page === 'nowPlaying'}
    <NowPlayingView />
  {:else if ui.page === 'radio'}
    <RadioView />
  {:else}
    <SettingsView />
  {/if}
</AppShell>

<OverlayHost />
