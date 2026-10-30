<script lang="ts">
  import { Radio, X } from '@lucide/svelte';
  import Button from '../components/ui/Button.svelte';
  import IconButton from '../components/ui/IconButton.svelte';
  import { errorMessage } from '../lib/errors';
  import { formatTime, transientOrigin } from '../lib/format';
  import { ipc } from '../lib/ipc';
  import { livePhase, rememberLive } from '../lib/live';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 直播页（docs/52 §7.3，原「FM 电台」占位页）。
   *
   * 只做三件事，**不做台站库**：
   * 1. 打开一个流（地址框 / 本地播放列表 / 拖文件进来，最后一条入口在壳层）；
   * 2. 最近打开 20 条（存设置 KV，按 URL 去重，不建表）；
   * 3. 正在听的那个台的大画面（标题 + 出处 + 已收听 + 三态）。
   *
   * 为什么没有频谱：正在播放页那套 `NowPlayingSpectrum` 绑着 `npSpectrum`
   * 开关与它自己的画布，搬到这里就是第二个 GL 上下文。直播页面前面就是
   * 全应用背景舞台（`AmbientBackdrop`），"正在响"这件事已经有人负责了。
   *
   * 也不做「码率选择」：用户拍板 **m3u8 给什么码率就播什么码率**
   * （master playlist 永远取第一个变体），这是播放器本体的行为，不是页面功能。
   */

  let draft = $state('');
  let busy = $state(false);

  const playerState = $derived(player.state);
  const isStream = $derived(playerState?.source === 'stream');
  const phase = $derived(livePhase(playerState?.status));
  /** 已收听时长：直播没有"当前位置"，`positionMs` 单调递增就是它 */
  const listened = $derived(playerState?.positionMs ?? 0);
  const title = $derived(playerState?.current?.title ?? '');
  const origin = $derived(transientOrigin(playerState?.current?.id));
  const recent = $derived(ui.settings.liveRecent ?? []);

  /** 写回「最近打开」。设置走 `applySettings`：失败会落到错误条，不静默 */
  function remember(url: string, name?: string): void {
    ui.applySettings({
      ...ui.settings,
      liveRecent: rememberLive(ui.settings.liveRecent ?? [], { url, ...(name ? { title: name } : {}) }),
    });
  }

  async function openUrl(raw: string, name?: string): Promise<void> {
    const url = raw.trim();
    if (!url) {
      notify.showToast('先填一个 m3u8 地址');
      return;
    }
    if (busy) return;
    busy = true;
    try {
      await ipc.live.open(url, name);
      remember(url, name);
      draft = '';
      notify.showToast(`正在连接 ${name ?? url}`);
    } catch (error) {
      // 显式动作失败 → 错误条（吐司 3 秒就没了，用户还得重新粘地址）
      notify.showError(errorMessage(error));
    } finally {
      busy = false;
    }
  }

  /**
   * 「打开文件…」：本地 m3u8/m3u 走**拖入那条链**（`player_open_paths`），
   * 不走 `live_open` —— 后者只收 http(s)（本地播放列表由宿主读盘，见 drop.rs）。
   */
  async function pickFile(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      const path = await ipc.live.pick();
      if (!path) return; // 取消不是错误
      const receipt = await ipc.player.openPaths([path]);
      if (receipt.opened === 0) {
        notify.showToast(`打不开：${receipt.skipped.join('；')}`, 6000);
        return;
      }
      notify.showToast('已打开播放列表');
    } catch (error) {
      notify.showError(errorMessage(error));
    } finally {
      busy = false;
    }
  }

  function forget(url: string): void {
    ui.applySettings({
      ...ui.settings,
      liveRecent: (ui.settings.liveRecent ?? []).filter((item) => item.url !== url),
    });
  }
</script>

<div class="live">
  <header class="live-head">
    <span class="page-kicker">LIVE</span>
    <h1>直播</h1>
    <p>粘一个 m3u8 地址，或打开本地播放列表。把文件拖进窗口也能直接播。</p>
  </header>

  <div class="live-form">
    <input
      class="live-input"
      bind:value={draft}
      placeholder="https://example.com/hls/index.m3u8"
      aria-label="直播流地址"
      spellcheck="false"
      autocomplete="off"
      onkeydown={(event) => {
        if (event.key === 'Enter') void openUrl(draft);
      }}
    />
    <Button variant="primary" disabled={busy || !draft.trim()} onclick={() => void openUrl(draft)}>
      打开
    </Button>
    <Button variant="ghost" disabled={busy} onclick={() => void pickFile()}>打开文件…</Button>
  </div>

  <!--
    大画面：回答"现在在听什么台"。没在播时也不空着——空态里放的是
    "入口在哪"，比一句"未在播放"有用。
  -->
  <section class="live-stage" class:on={isStream && phase.tone === 'on'}>
    {#if isStream}
      <span class="stage-badge" class:on={phase.tone === 'on'} class:wait={phase.tone === 'wait'}>
        <span class="stage-dot" aria-hidden="true"></span>{phase.text}
      </span>
      <h2 class="stage-title">{title}</h2>
      <p class="stage-origin">{origin ?? '网络直播流'}</p>
      <p class="stage-time" title="已收听">{formatTime(listened)}</p>
      <p class="stage-hint">
        暂停会断开连接，再按播放重新连上——直播没有"从中间接着听"这回事。
      </p>
    {:else}
      <span class="stage-icon"><Radio size={30} strokeWidth={1.4} /></span>
      <h2 class="stage-title">还没有在听直播</h2>
      <p class="stage-hint">上面填地址，或者打开一个本地 m3u8 播放列表。</p>
    {/if}
  </section>

  <section class="live-recent">
    <div class="recent-head">
      <strong>最近打开</strong>
      <span class="recent-count">{recent.length} / 20</span>
    </div>
    {#if recent.length === 0}
      <p class="recent-empty">还没有记录。打开过的地址会留在这里，按地址去重。</p>
    {:else}
      <ul class="recent-list">
        {#each recent as item (item.url)}
          <li class="recent-row">
            <button
              class="recent-open"
              onclick={() => void openUrl(item.url, item.title)}
              title={item.url}
            >
              <span class="recent-title">{item.title ?? item.url}</span>
              <span class="recent-url">{item.url}</span>
            </button>
            <IconButton label="从最近打开中移除" size="sm" onclick={() => forget(item.url)}>
              <X size={12} />
            </IconButton>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .live {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: min(820px, 100%);
    margin: 0 auto;
    padding: 18px 4px 8px;
  }

  .live-head h1 {
    margin: 4px 0 2px;
    font-size: 22px;
    color: var(--text-1);
  }

  .live-head p {
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }

  /* ---------------- 地址框 ---------------- */

  .live-form {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .live-input {
    flex: 1;
    min-width: 0;
    height: var(--control-h);
    padding: 0 12px;
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
    background: var(--surface-2);
    color: var(--text-1);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .live-input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    border-color: transparent;
  }

  /* ---------------- 大画面 ---------------- */

  /*
   * 亮/暗两套主题都靠 CSS 变量走，不写死颜色。底色是一层极淡的放射渐变：
   * 直播是"有东西在流动"的场景，一块死平的卡片会让人以为页面没加载完。
   */
  .live-stage {
    display: grid;
    justify-items: center;
    gap: 6px;
    padding: 34px 24px 30px;
    border-radius: var(--radius-lg);
    border: 1px solid var(--line);
    background:
      radial-gradient(
        120% 90% at 50% 0%,
        color-mix(in srgb, var(--accent) 10%, transparent),
        transparent 70%
      ),
      color-mix(in srgb, var(--surface-2) 88%, transparent);
    text-align: center;
  }

  .stage-icon {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 16px;
    background: var(--surface-3);
    border: 1px solid var(--line);
    color: var(--text-3);
  }

  .stage-title {
    margin: 4px 0 0;
    font-size: 19px;
    font-weight: 560;
    color: var(--text-1);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stage-origin {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-3);
  }

  /* 已收听时长：等宽数字，别让秒位跳动时整行左右抖 */
  .stage-time {
    margin: 6px 0 0;
    font-size: 26px;
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    letter-spacing: 0.02em;
  }

  .stage-hint {
    margin: 4px 0 0;
    max-width: 46ch;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-3);
  }

  .stage-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 9px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--line-strong);
    font-size: 10.5px;
    letter-spacing: 0.04em;
    color: var(--text-3);
  }

  .stage-badge.wait {
    color: var(--text-2);
  }

  .stage-badge.on {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  .stage-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  /* ---------------- 最近打开 ---------------- */

  .recent-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 8px;
    font-size: 12.5px;
    color: var(--text-1);
  }

  .recent-count {
    font-size: 10.5px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .recent-empty {
    margin: 0;
    font-size: 11.5px;
    color: var(--text-3);
  }

  .recent-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .recent-row {
    display: flex;
    align-items: center;
    gap: 6px;
    border-radius: var(--radius-sm);
    padding-right: 4px;
  }

  .recent-row:hover {
    background: var(--surface-hover);
  }

  .recent-open {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 1px;
    padding: 7px 8px;
    text-align: left;
  }

  .recent-title {
    font-size: 12px;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .recent-url {
    font-size: 10.5px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
