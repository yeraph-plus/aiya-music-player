<script lang="ts">
  import { ChevronLeft, ChevronRight, Plus, Radio, X } from '@lucide/svelte';
  import { fly } from 'svelte/transition';
  import Button from '../components/ui/Button.svelte';
  import Dialog from '../components/ui/Dialog.svelte';
  import Field from '../components/ui/Field.svelte';
  import IconButton from '../components/ui/IconButton.svelte';
  import { errorMessage } from '../lib/errors';
  import { formatTime } from '../lib/format';
  import { ipc } from '../lib/ipc';
  import { LIVE_SWAP_MS, livePhase, rememberLive, streamUrlOf } from '../lib/live';
  import { reducedMotion } from '../lib/motion.svelte';
  import type { LiveRecentEntry } from '../lib/settings';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 直播页：台卡轮播。
   *
   * 页面只做四件事：
   * 1. 卡组浏览——左右箭头（或方向键）在台与台之间切换，动效是三张卡绕
   *    Y 轴的轮转（角色 class 驱动 CSS 变量，无 JS 动画库）；
   * 2. 开播——点中央卡连流；侧卡点击只负责把它转进来；
   * 3. 新增台——模态框录入地址与名称；台库为空时中央常驻一张加号卡，
   *    点它就是开这个弹窗（没有单独的空态占位层，卡组就是页面本身）；
   * 4. 移除台——卡右上角的悬停按钮。
   *
   * 台库就是「最近打开」（`liveRecent`，按 URL 去重、上限 20）：开播过或
   * 手动添加过的地址都是台，不另建数据。
   */

  let browseIndex = $state(0);
  /** 换台过渡进行中：锁住箭头与方向键，卡片不叠影 */
  let swapLock = $state(false);
  /** 换台方向：过渡期间决定哪张卡从最前层掠过 */
  let swapDir = $state<'left' | 'right' | null>(null);
  let swapTimer: number | undefined;
  let busy = $state(false);

  let adding = $state(false);
  let ghostIn = $state(false);
  let draftUrl = $state('');
  let draftName = $state('');
  let draftError = $state<string | null>(null);
  let urlFieldRef = $state<HTMLInputElement | null>(null);

  const playerState = $derived(player.state);
  const recent = $derived(ui.settings.liveRecent ?? []);
  /** 正在播的流地址：非直播（本地曲目）时为 null，卡组里就没有"正在播"的卡 */
  const playingUrl = $derived(streamUrlOf(playerState?.current?.id));
  const phase = $derived(livePhase(playerState?.status));
  /** 已收听时长：直播没有"当前位置"，`positionMs` 单调递增就是它 */
  const listened = $derived(playerState?.positionMs ?? 0);
  const currentEntry = $derived(recent.length ? recent[modIndex(browseIndex, recent.length)] : null);

  function modIndex(index: number, n: number): number {
    return ((index % n) + n) % n;
  }

  /**
   * 卡在卡组里的角色：0 → 中央，1 → 右（next），n-1 → 左（previous），
   * 其余藏在 previous 槽位淡出，被轮到时原地淡入。
   */
  function roleOf(index: number): 'role-current' | 'role-next' | 'role-previous' | 'role-hidden' {
    const n = recent.length;
    const offset = modIndex(index - browseIndex, n);
    if (offset === 0) return 'role-current';
    if (offset === 1) return 'role-next';
    if (offset === n - 1) return 'role-previous';
    return 'role-hidden';
  }

  /** 卡面与信息块显示的台名：名称优先，缺省回落到主机名 */
  function stationName(entry: LiveRecentEntry): string {
    if (entry.title) return entry.title;
    return hostOf(entry.url);
  }

  function hostOf(url: string): string {
    try {
      return new URL(url).host || url;
    } catch {
      return url;
    }
  }

  function cardAria(entry: LiveRecentEntry, index: number): string {
    const name = stationName(entry);
    return roleOf(index) === 'role-current' ? `播放 ${name}` : `切换到 ${name}`;
  }

  /** 浏览到第 index 张卡。`dir` 只在步进换台时给出，用于层级编排 */
  function browseTo(index: number, dir: 'left' | 'right' | null = null): void {
    const n = recent.length;
    if (n < 2 || swapLock || adding) return;
    const next = modIndex(index, n);
    if (next === browseIndex) return;
    swapLock = true;
    swapDir = dir;
    browseIndex = next;
    window.clearTimeout(swapTimer);
    swapTimer = window.setTimeout(() => {
      swapLock = false;
      swapDir = null;
    }, LIVE_SWAP_MS);
  }

  function swap(dir: 'left' | 'right'): void {
    browseTo(browseIndex + (dir === 'right' ? 1 : -1), dir);
  }

  function onCardClick(entry: LiveRecentEntry, index: number): void {
    if (roleOf(index) !== 'role-current') {
      browseTo(index);
      return;
    }
    // 正在播这个台时再点一次不是重连，直接忽略
    if (entry.url === playingUrl && playerState?.status === 'playing') return;
    void openUrl(entry.url, entry.title);
  }

  /** 悬停倾斜：只作用于中央卡，随鼠标横移绕 Y 轴摆动。变量写在卡壳上（transform 的载体） */
  function tiltCard(event: PointerEvent, index: number): void {
    if (reducedMotion.value || roleOf(index) !== 'role-current') return;
    const face = event.currentTarget as HTMLElement;
    const card = face.parentElement;
    if (!card) return;
    const box = face.getBoundingClientRect();
    const angle = (Math.atan2(event.clientX - (box.left + box.width / 2), 0) * 35) / Math.PI;
    card.style.setProperty('--live-tilt', `${angle.toFixed(2)}deg`);
  }

  function untiltCard(event: PointerEvent): void {
    (event.currentTarget as HTMLElement).parentElement?.style.removeProperty('--live-tilt');
  }

  function onKeydown(event: KeyboardEvent): void {
    if (adding || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key === 'ArrowLeft') swap('left');
    else if (event.key === 'ArrowRight') swap('right');
  }

  function openAdd(): void {
    draftUrl = '';
    draftName = '';
    draftError = null;
    adding = true;
  }

  function closeAdd(): void {
    adding = false;
  }

  function submitAdd(): void {
    const url = draftUrl.trim();
    if (!/^https?:\/\//i.test(url)) {
      draftError = '仅支持 http(s) 地址';
      return;
    }
    const name = draftName.trim();
    // 重复添加同一个地址且没填新名称时，沿用已有的台名，不让空名称覆盖旧值
    const existing = recent.find((item) => item.url === url);
    const title = name || existing?.title;
    const exists = Boolean(existing);
    ui.applySettings({
      ...ui.settings,
      liveRecent: rememberLive(recent, { url, ...(title ? { title } : {}) }),
    });
    adding = false;
    // rememberLive 把新台放到卡组头部，浏览位归零它就在中央
    browseIndex = 0;
    notify.showToast(exists ? `已在卡组中：${name || url}` : '已添加电台');
  }

  /** 写回「最近打开」。设置走 `applySettings`：失败会落到错误条，不静默 */
  function remember(url: string, name?: string): void {
    ui.applySettings({
      ...ui.settings,
      liveRecent: rememberLive(ui.settings.liveRecent ?? [], { url, ...(name ? { title: name } : {}) }),
    });
  }

  async function openUrl(url: string, name?: string): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await ipc.live.open(url, name);
      remember(url, name);
      // 开台的卡被置顶后浏览位跟过去，中央卡始终是正在播的这个台
      const index = (ui.settings.liveRecent ?? []).findIndex((item) => item.url === url);
      if (index >= 0) browseIndex = index;
      notify.showToast(`正在连接 ${name ?? url}`);
    } catch (error) {
      // 开台失败不阻断运行：走播放条吐司（报错三档，2026-11 拍板）。
      // 地址还留在卡组与地址框里，吐司消失后重试成本就是点一下。
      notify.showToast(errorMessage(error));
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
    const list = ui.settings.liveRecent ?? [];
    const index = list.findIndex((item) => item.url === url);
    if (index < 0) return;
    ui.applySettings({
      ...ui.settings,
      liveRecent: list.filter((item) => item.url !== url),
    });
    const rest = list.length - 1;
    if (rest === 0) {
      browseIndex = 0;
      return;
    }
    // 移除位在浏览位左侧时前移一位；正好是被浏览的卡就停在原槽位看下一位
    if (index < browseIndex) browseIndex -= 1;
    else if (index === browseIndex) browseIndex = modIndex(browseIndex, rest);
  }

  // 透明卡入场：先以停放位（视口下方）挂载，下一帧再翻入槽位，transition 才有起点
  $effect(() => {
    if (!adding) {
      ghostIn = false;
      return;
    }
    const raf = requestAnimationFrame(() => {
      ghostIn = true;
    });
    return () => cancelAnimationFrame(raf);
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="live">
  <header class="live-head">
    <div>
      <span class="page-kicker">LIVE</span>
      <h1>直播</h1>
      <p>左右换台浏览，点中央卡开播。把 m3u8 文件拖进窗口也能直接播。</p>
    </div>
    <div class="live-actions">
      <Button variant="primary" onclick={openAdd}>
        {#snippet icon()}<Plus size={14} />{/snippet}
        添加台
      </Button>
      <Button variant="ghost" disabled={busy} onclick={() => void pickFile()}>打开文件…</Button>
    </div>
  </header>

  <section class="deck">
    <div
      class="stage"
      class:locked={swapLock}
      class:dir-left={swapDir === 'left'}
      class:dir-right={swapDir === 'right'}
      style="--live-swap-ms: {LIVE_SWAP_MS}ms"
    >
      {#each recent as entry, i (entry.url)}
        <div class="live-card {roleOf(i)}">
          <button
            class="card-face"
            onclick={() => onCardClick(entry, i)}
            onpointermove={(e) => tiltCard(e, i)}
            onpointerleave={untiltCard}
            aria-label={cardAria(entry, i)}
          >
            <span class="face-mark" aria-hidden="true"><Radio size={48} strokeWidth={1.2} /></span>
            <span class="face-name">{stationName(entry)}</span>
          </button>
          <span class="card-remove">
            <IconButton label="从最近打开中移除" size="sm" onclick={() => forget(entry.url)}>
              <X size={12} />
            </IconButton>
          </span>
        </div>
      {/each}

      <!-- 空态：加号卡常驻中央，点它打开新增弹窗；没有单独的空态占位层 -->
      {#if recent.length === 0}
        <button class="live-card ghost ghost-add role-current ghost-in" onclick={openAdd} aria-label="添加台">
          <span class="card-face ghost-face">
            <Plus size={26} strokeWidth={1.6} />
            <span class="ghost-text">添加台</span>
          </span>
        </button>
      {:else if adding}
        <!-- 新增中的透明卡：从 next 槽位滑入卡组 -->
        <div class="live-card ghost role-next" class:ghost-in={ghostIn} aria-hidden="true">
          <div class="card-face ghost-face">
            <Plus size={26} strokeWidth={1.6} />
            <span class="ghost-text">新台</span>
          </div>
        </div>
      {/if}

      <div class="deck-arrow arrow-left">
        <IconButton label="上一台" size="lg" disabled={recent.length < 2} onclick={() => swap('left')}>
          <ChevronLeft size={20} />
        </IconButton>
      </div>
      <div class="deck-arrow arrow-right">
        <IconButton label="下一台" size="lg" disabled={recent.length < 2} onclick={() => swap('right')}>
          <ChevronRight size={20} />
        </IconButton>
      </div>

      {#if currentEntry}
        {@const currentUrl = currentEntry.url}
        {#key currentUrl}
          <div
            class="live-info"
            in:fly={{ y: 20, duration: reducedMotion.value ? 0 : 320, delay: reducedMotion.value ? 0 : 180 }}
            out:fly={{ y: -16, duration: reducedMotion.value ? 0 : 180 }}
          >
            <h2 class="info-name">{stationName(currentEntry)}</h2>
            <p class="info-host">{hostOf(currentUrl)}</p>
            <p class="info-status">
              {#if playingUrl === currentUrl}
                <span class="stage-badge" class:on={phase.tone === 'on'} class:wait={phase.tone === 'wait'}>
                  <span class="stage-dot" aria-hidden="true"></span>{phase.text}
                </span>
                <span class="info-listened">{formatTime(listened)}</span>
              {:else}
                点卡片开播
              {/if}
            </p>
          </div>
        {/key}
      {/if}
    </div>
  </section>
</div>

{#if adding}
  <Dialog
    title="添加台"
    description="填一个 http(s) 的 m3u8 地址。名称可以留空，卡片会显示主机名。"
    autofocus={() => urlFieldRef?.focus()}
    onclose={closeAdd}
  >
    <div class="add-form">
      <Field
        label="m3u8 地址"
        bind:ref={urlFieldRef}
        value={draftUrl}
        error={draftError}
        mono
        placeholder="https://example.com/hls/index.m3u8"
        oninput={(value) => {
          draftUrl = value;
          draftError = null;
        }}
        onenter={submitAdd}
      />
      <Field
        label="名称（可选）"
        value={draftName}
        maxlength={80}
        placeholder="留空则显示主机名"
        oninput={(value) => (draftName = value)}
        onenter={submitAdd}
      />
    </div>
    {#snippet actions()}
      <Button onclick={closeAdd}>取消</Button>
      <Button variant="primary" disabled={!draftUrl.trim()} onclick={submitAdd}>添加</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .live {
    --live-card-w: 250px;
    --live-card-h: 400px;

    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 18px 22px 8px;
  }

  /* 内容区高度吃紧时卡组缩一档，头部与信息块不动 */
  @media (max-height: 799px) {
    .live {
      --live-card-w: 200px;
      --live-card-h: 320px;
    }
  }

  .live-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
  }

  .live-head h1 {
    margin: 4px 0 2px;
    font-size: var(--fs-2xl);
    color: var(--text-1);
  }

  .live-head p {
    margin: 0;
    font-size: var(--fs-base);
    color: var(--text-2);
  }

  .live-actions {
    display: flex;
    flex: none;
    gap: 8px;
    padding-bottom: 2px;
  }

  /* 卡组 */

  .deck {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* perspective 自成堆叠上下文：卡与箭头的层级只在台内比较，取值全部来自 --z-* 阶梯 */
  .stage {
    position: relative;
    width: calc(var(--live-card-w) * 3);
    height: var(--live-card-h);
    perspective: 1000px;
  }

  .live-card {
    --card-translateX-offset: 0px;
    --card-rotation-offset: 0deg;
    --card-scale-offset: 1;
    /* 黑罩压暗强度：中央 20%，两侧 60% */
    --card-dim: 0.2;
    /* 停放位：透明卡藏起来的地方（视口下方），真卡恒为 0 */
    --card-translateY-offset: 0%;

    position: absolute;
    left: 50%;
    top: 50%;
    width: var(--live-card-w);
    height: var(--live-card-h);
    transform: translate(-50%, -50%) translateX(var(--card-translateX-offset))
      translateY(var(--card-translateY-offset)) rotateY(var(--card-rotation-offset))
      scale(var(--card-scale-offset));
    transition:
      transform var(--live-swap-ms, 800ms) var(--ease),
      opacity var(--live-swap-ms, 800ms) var(--ease);
    z-index: var(--z-content);
  }

  .live-card.role-current {
    --card-rotation-offset: var(--live-tilt, 0deg);
    --card-scale-offset: 1.2;
  }

  .live-card.role-next {
    --card-translateX-offset: calc(var(--live-card-w) * 1.1);
    --card-rotation-offset: -25deg;
    --card-scale-offset: 0.9;
    --card-dim: 0.6;
    z-index: var(--z-bg);
  }

  .live-card.role-previous {
    --card-translateX-offset: calc(var(--live-card-w) * -1.1);
    --card-rotation-offset: 25deg;
    --card-scale-offset: 0.9;
    --card-dim: 0.6;
    z-index: var(--z-bg);
  }

  /* 递补位：停在 previous 槽位收小淡出，被轮到时原地长回来 */
  .live-card.role-hidden {
    --card-translateX-offset: calc(var(--live-card-w) * -1.1);
    --card-rotation-offset: 25deg;
    --card-scale-offset: 0.6;
    --card-dim: 0.85;
    opacity: 0;
    pointer-events: none;
    z-index: var(--z-under);
  }

  /* 换台瞬间的层级编排：离场卡从最前层掠过，入场卡（新的 current）次之 */
  .stage.dir-right .live-card.role-previous,
  .stage.dir-left .live-card.role-next {
    z-index: var(--z-sticky);
  }

  .card-face {
    position: relative;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 12px;
    width: 100%;
    height: 100%;
    padding: 18px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background:
      radial-gradient(
        120% 90% at 50% 0%,
        color-mix(in srgb, var(--accent) 24%, transparent),
        transparent 72%
      ),
      var(--surface-3);
    color: var(--text-1);
    box-shadow: var(--shadow-3);
    cursor: pointer;
    overflow: hidden;
  }

  /* 黑罩压暗：罩住整张卡面（含台名），侧卡因此退成剪影 */
  .card-face::after {
    content: '';
    position: absolute;
    inset: 0;
    background: #000;
    opacity: var(--card-dim);
    transition: opacity var(--live-swap-ms, 800ms) var(--ease);
    pointer-events: none;
  }

  .face-mark {
    color: color-mix(in srgb, var(--accent) 46%, transparent);
  }

  .face-name {
    max-width: 100%;
    overflow: hidden;
    font-size: var(--fs-lg);
    font-weight: 640;
    text-align: center;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-remove {
    position: absolute;
    top: 8px;
    right: 8px;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
    z-index: var(--z-dropline);
  }

  .live-card:hover .card-remove,
  .card-remove:focus-visible {
    opacity: 1;
  }

  /* 新增中的透明卡 */
  .live-card.ghost {
    --card-dim: 0;
    --card-translateY-offset: 100vh;
    opacity: 0;
    pointer-events: none;
    z-index: var(--z-bg);
  }

  .live-card.ghost.ghost-in {
    --card-translateY-offset: 0%;
    opacity: 1;
  }

  .ghost-face {
    border-style: dashed;
    border-color: var(--line-strong);
    background: color-mix(in srgb, var(--surface-3) 45%, transparent);
    box-shadow: none;
    color: var(--text-3);
  }

  .ghost-face::after {
    content: none;
  }

  .ghost-text {
    font-size: var(--fs-sm);
  }

  /* 箭头 */

  .deck-arrow {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    transition: opacity var(--dur-base) var(--ease);
    z-index: var(--z-dropline);
  }

  .deck-arrow.arrow-left {
    left: -5%;
  }

  .deck-arrow.arrow-right {
    right: -5%;
  }

  .stage.locked .deck-arrow {
    opacity: 0.5;
    pointer-events: none;
  }

  /* 信息块：贴着中央卡左下沿，文字压在卡面上（指针事件让给卡） */

  .live-info {
    position: absolute;
    left: calc(var(--live-card-w) / 1.5);
    bottom: calc(var(--live-card-h) / 8);
    display: grid;
    gap: 2px;
    max-width: calc(var(--live-card-w) * 1.4);
    pointer-events: none;
    z-index: var(--z-content);
  }

  .info-name {
    margin: 0;
    overflow: hidden;
    font-size: var(--fs-xl);
    font-weight: 680;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-1);
  }

  .info-host {
    --mg-left: 40px;

    position: relative;
    margin: 0 0 0 var(--mg-left);
    padding-bottom: 0.5rem;
    overflow: hidden;
    font-size: var(--fs-sm);
    font-weight: 620;
    letter-spacing: 0.04em;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-1);
  }

  /* 台名前的短横与下划线（模板的 location 装饰） */
  .info-host::before,
  .info-host::after {
    content: '';
    position: absolute;
    background: currentColor;
  }

  .info-host::before {
    top: 50%;
    left: 0;
    width: 20px;
    height: 5px;
    transform: translate(calc(-1 * var(--mg-left) - 6px), -50%);
  }

  .info-host::after {
    bottom: 0;
    left: 0;
    width: 60px;
    height: 2px;
    transform: translate(calc(-1 * var(--mg-left)), 0);
  }

  .info-status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 0 0;
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .info-listened {
    font-variant-numeric: tabular-nums;
  }

  .stage-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 9px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--line-strong);
    font-size: var(--fs-xs);
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

  /* 空态的加号卡：它是真按钮（点=开新增弹窗），要盖掉 .ghost 的 pointer-events:none
     并复位 button 默认样式（壳上的定位/变换都由 .live-card 提供） */
  .live-card.ghost-add {
    appearance: none;
    border: none;
    padding: 0;
    background: none;
    font: inherit;
    pointer-events: auto;
    cursor: pointer;
  }

  .ghost-add:hover .ghost-face,
  .ghost-add:focus-visible .ghost-face {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
    color: var(--text-2);
  }

  /* 模态框表单 */

  .add-form {
    display: grid;
    gap: 12px;
    padding: 14px 16px;
  }

  @media (prefers-reduced-motion: reduce) {
    .live-card,
    .card-face::after,
    .deck-arrow {
      transition-duration: 0s;
    }
  }
</style>
