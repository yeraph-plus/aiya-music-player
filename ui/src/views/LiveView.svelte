<script lang="ts">
  import { AudioLines, ChevronLeft, ChevronRight, Plus, Radio, X } from '@lucide/svelte';
  import { fly } from 'svelte/transition';
  import Button from '../components/ui/Button.svelte';
  import Dialog from '../components/ui/Dialog.svelte';
  import Field from '../components/ui/Field.svelte';
  import Header from '../components/content/Header.svelte';
  import IconButton from '../components/ui/IconButton.svelte';
  import { errorMessage } from '../lib/errors';
  import { formatTime } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { ipc } from '../lib/ipc';
  import { backendCaps } from '../lib/backend-caps';
import {
  LIVE_SWAP_MS,
  livePhase,
  mergeServiceStations,
  rememberLive,
  streamUrlOf,
} from '../lib/live';
import StationPanel from '../components/station/StationPanel.svelte';
import TextTabs from '../components/ui/TextTabs.svelte';
  import { reducedMotion } from '../lib/motion.svelte';
  import type { LiveRecentEntry } from '../lib/settings';
  import { notify } from '../stores/notify.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 直播页：流卡轮播（层叠式）。
   *
   * 页面只做四件事：
   * 1. 卡组浏览——焦点卡居中，其余的流**按顺序向两侧层叠**（越深越靠后、
   *    越小越淡，超过可见深度整体隐去但仍占位，轮到时从原位长回来），
   *    左右箭头（或方向键）移动焦点；「添加流」卡永远排在队尾。
   * 2. 开播——点焦点卡连流；侧卡点击只负责把它转进来。
   * 3. 新增流——队尾的加号卡与头部按钮都开同一个模态框；卡面只画图标
   *    或封面，标题/副标题/描述在左下悬浮信息层。
   * 4. 移除流——卡右上角的悬停按钮。
   *
   * 流库就是「最近打开」（`liveRecent`，按 URL 去重、上限 20）：开播过或
   * 手动添加过的地址都是流，不另建数据。卡面文字不上卡是刻意的：卡只负责
   * "这是哪条流"（图/封面），文字细节归悬浮层，左对齐阅读。
   */

  /** 两侧各最多露出的层叠深度；更深的卡整体隐去但仍占位（轮到时从原位回来） */
  const STACK_DEPTH = 3;

  /** 双 tab（docs/68 §12.3）：收听（现状卡组）/ 我的频道（推流管理器）。
   *  管理器只在 web 宿主渲染（caps.streaming——桌面壳没有 station 命令）。 */
  let tab = $state<'listen' | 'manage'>('listen');
  const caps = backendCaps();

  /** 站单下发：服务端频道列表合并进收听列表（source='service'，用户字段赢）。
   *  挂载时拉一次；管理器每次变更（开播/停播/元数据保存）后由 onchanged 再拉。 */
  async function pullStations(): Promise<void> {
    if (!caps.streaming) return;
    try {
      const list = await ipc.station.list();
      ui.applySettings({
        ...ui.settings,
        liveRecent: mergeServiceStations(ui.settings.liveRecent ?? [], list),
      });
    } catch {
      // 站单是增强面：拉不到不影响收听列表本身
    }
  }

  const liveTabs = $derived([
    { id: 'listen', label: t('station.tabListen') },
    { id: 'manage', label: t('station.tabManage') },
  ]);

  let browseIndex = $state(0);
  /** 换流过渡进行中：锁住箭头与方向键，卡片不叠影 */
  let swapLock = $state(false);
  let swapTimer: number | undefined;
  let busy = $state(false);

  let adding = $state(false);
  let draftUrl = $state('');
  let draftName = $state('');
  let draftSubtitle = $state('');
  let draftDescription = $state('');
  let draftError = $state<string | null>(null);
  let urlFieldRef = $state<HTMLInputElement | null>(null);

  const playerState = $derived(player.state);
  const recent = $derived(ui.settings.liveRecent ?? []);
  /** 卡组总长 = 流数 + 队尾常驻的「添加流」卡 */
  const deckLen = $derived(recent.length + 1);
  /** 正在播的流地址：非直播（本地曲目）时为 null */
  const playingUrl = $derived(streamUrlOf(playerState?.current?.id));
  const phase = $derived(livePhase(playerState?.status));
  /** 已收听时长：直播没有"当前位置"，`positionMs` 单调递增就是它 */
  const listened = $derived(playerState?.positionMs ?? 0);
  /** 焦点流；焦点落在队尾加号卡上时为 null */
  const currentEntry = $derived(browseIndex < recent.length ? recent[browseIndex] : null);

  /**
   * 卡在卡组里的槽位。**不循环**：offset = index - browseIndex，负数在左
   * （已过）、正数在右（未到），绝绝对对的双向序列，到头就是空的。
   * 深度超过可见上限的卡仍然定位（只是整体隐去），折返时从原位长回来。
   */
  function slotOf(index: number): { kind: 'current' } | { kind: 'side'; side: 'left' | 'right'; depth: number; out: boolean } {
    const offset = index - browseIndex;
    if (offset === 0) return { kind: 'current' };
    const side = offset < 0 ? 'left' : 'right';
    const depth = Math.abs(offset);
    return { kind: 'side', side, depth, out: depth > STACK_DEPTH };
  }

  function slotClass(index: number): string {
    const slot = slotOf(index);
    if (slot.kind === 'current') return 'pos-current';
    const clamped = Math.min(slot.depth, STACK_DEPTH);
    return `${slot.side === 'right' ? 'side-right' : 'side-left'} depth-${clamped}${slot.out ? ' depth-out' : ''}`;
  }

  /** 层内 z：焦点最高，越深越低。perspective 让 .stage 自成堆叠上下文，
   *  这些值只在舞台堆叠上下文内比较，不与 --z-* 阶梯混用 */
  function slotZ(index: number): number {
    const slot = slotOf(index);
    return slot.kind === 'current' ? 30 : 30 - Math.min(slot.depth, STACK_DEPTH) * 10;
  }

  /** 卡面与信息层显示的流名：名称优先，缺省回落到主机名 */
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

  function cardAria(entry: LiveRecentEntry): string {
    return t('live.cardPlayAria', { name: stationName(entry) });
  }

  /** 浏览到第 index 张卡（含队尾加号卡）。**不循环**：越界即停在端点 */
  function browseTo(index: number): void {
    if (deckLen < 2 || swapLock || adding) return;
    const next = Math.max(0, Math.min(deckLen - 1, index));
    if (next === browseIndex) return;
    swapLock = true;
    browseIndex = next;
    window.clearTimeout(swapTimer);
    swapTimer = window.setTimeout(() => {
      swapLock = false;
    }, LIVE_SWAP_MS);
  }

  function swap(dir: 'left' | 'right'): void {
    browseTo(browseIndex + (dir === 'right' ? 1 : -1));
  }

  /** 只有焦点卡可点（侧卡整体 inert）：再点一次正在播的流不是重连，忽略 */
  function onCardClick(entry: LiveRecentEntry): void {
    if (entry.url === playingUrl && playerState?.status === 'playing') return;
    void openUrl(entry.url, entry.title);
  }

  /** 队尾加号卡：转进来之前只负责转进来；已是焦点才开弹窗 */
  function onAddCardClick(): void {
    const idx = recent.length;
    if (browseIndex !== idx) {
      browseTo(idx);
      return;
    }
    openAdd();
  }

  /** 悬停倾斜：只作用于焦点卡，随鼠标横移绕 Y 轴摆动。变量写在卡壳上（transform 的载体） */
  function tiltCard(event: PointerEvent, index: number): void {
    if (reducedMotion.value || slotOf(index).kind !== 'current') return;
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
    draftSubtitle = '';
    draftDescription = '';
    draftError = null;
    adding = true;
  }

  function closeAdd(): void {
    adding = false;
  }

  function submitAdd(): void {
    const url = draftUrl.trim();
    if (!/^https?:\/\//i.test(url)) {
      draftError = t('live.errorHttpOnly');
      return;
    }
    const name = draftName.trim();
    const subtitle = draftSubtitle.trim();
    const description = draftDescription.trim();
    // 字段级合并交给 rememberLive（新值优先、缺位保旧）：重填旧地址补
    // 副标题时，名称留空不会把已有流名抹掉
    const exists = recent.some((item) => item.url === url);
    ui.applySettings({
      ...ui.settings,
      liveRecent: rememberLive(recent, {
        url,
        ...(name ? { title: name } : {}),
        ...(subtitle ? { subtitle } : {}),
        ...(description ? { description } : {}),
      }),
    });
    adding = false;
    // rememberLive 把新流放到卡组头部，浏览位归零它就在中央
    browseIndex = 0;
    notify.showToast(exists ? t('live.toastExists', { name: name || url }) : t('live.toastAdded'));
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
      // 开流的卡被置顶后浏览位跟过去，焦点卡始终是正在播的这一条
      const index = (ui.settings.liveRecent ?? []).findIndex((item) => item.url === url);
      if (index >= 0) browseIndex = index;
      notify.showToast(t('live.toastConnecting', { name: name ?? url }));
    } catch (error) {
      // 开流失败不阻断运行：走播放条吐司（报错三档，2026-11 拍板）。
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
        notify.showToast(t('live.toastOpenFailed', { reasons: receipt.skipped.join(t('lib.drop.reasonsSep')) }), 6000);
        return;
      }
      notify.showToast(t('live.toastPlaylistOpened'));
    } catch (error) {
      notify.showError(errorMessage(error));
    } finally {
      busy = false;
    }
  }

  // 站单首拉：web 形态挂载即拉一次（服务端频道卡片自动进收听列表）
  $effect(() => {
    void pullStations();
  });

  /** 按稳定身份移除（不是按 URL）：将来流的地址会变，身份不变 */
  function forget(id: string): void {
    const list = ui.settings.liveRecent ?? [];
    const index = list.findIndex((item) => item.id === id);
    if (index < 0) return;
    ui.applySettings({
      ...ui.settings,
      liveRecent: list.filter((item) => item.id !== id),
    });
    // 流数减少后卡组总长随之缩一；焦点越界就收回到最后一张有效卡
    const deckAfter = list.length + 1;
    if (browseIndex > deckAfter - 1) browseIndex = deckAfter - 1;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="live">
  <Header kicker="LIVE" title={t('live.title')} subtitle={t('live.subtitle')}>
    {#snippet actions()}
      <Button variant="primary" onclick={openAdd}>
        {#snippet icon()}<Plus size={14} />{/snippet}
        {t('live.addStream')}
      </Button>
      <Button variant="ghost" disabled={busy} onclick={() => void pickFile()}>{t('live.openFile')}</Button>
    {/snippet}
  </Header>

  {#if caps.streaming}
    <div class="live-tabs">
      <TextTabs
        items={liveTabs}
        value={tab}
        label={t('station.tabAria')}
        onchange={(id) => {
          tab = id as 'listen' | 'manage';
          if (tab === 'manage') void pullStations();
        }}
      />
    </div>
  {/if}

  {#if tab === 'manage'}
    <StationPanel onchanged={() => void pullStations()} />
  {:else}
  <section class="deck">
    <div
      class="stage"
      class:locked={swapLock}
      style="--live-swap-ms: {LIVE_SWAP_MS}ms"
    >
      {#each recent as entry, i (entry.id)}
        <!-- 非焦点卡整体 inert：切换只走两侧按钮，侧卡是纯视觉（不可点、
             不进焦点链、不进读屏树），删除也只在焦点卡上做 -->
        <div class="live-card {slotClass(i)}" style:z-index={slotZ(i)} inert={slotOf(i).kind !== 'current'}>
          <span class="card-mark" aria-hidden="true">
            <Radio size={12} />
          </span>
          <button
            class="card-face"
            onclick={() => onCardClick(entry)}
            onpointermove={(e) => tiltCard(e, i)}
            onpointerleave={untiltCard}
            aria-label={cardAria(entry)}
          >
            {#if entry.cover}
              <img class="face-cover" src={entry.cover} alt="" loading="lazy" />
            {:else}
              <span class="face-mark" aria-hidden="true"><AudioLines size={48} strokeWidth={1.2} /></span>
            {/if}
          </button>
          <span class="card-remove">
            <IconButton label={t('live.removeRecent')} size="sm" onclick={() => forget(entry.id)}>
              <X size={12} />
            </IconButton>
          </span>
        </div>
      {/each}

      <!-- 队尾常驻加号卡：转进来之前只负责转进来；已是焦点才开弹窗 -->
      <button
        class="live-card add-card {slotClass(recent.length)}"
        style:z-index={slotZ(recent.length)}
        onclick={onAddCardClick}
        aria-label={t('live.addStream')}
      >
        <span class="card-face add-face">
          <Plus size={30} strokeWidth={1.6} />
          <span class="add-text">{t('live.addStream')}</span>
        </span>
      </button>

      <div class="deck-arrow arrow-left">
        <IconButton label={t('live.prevStream')} size="lg" disabled={browseIndex === 0} onclick={() => swap('left')}>
          <ChevronLeft size={28} />
        </IconButton>
      </div>
      <div class="deck-arrow arrow-right">
        <IconButton label={t('live.nextStream')} size="lg" disabled={browseIndex === deckLen - 1} onclick={() => swap('right')}>
          <ChevronRight size={28} />
        </IconButton>
      </div>

      {#if currentEntry}
        {@const currentUrl = currentEntry.url}
        {#key currentEntry.id}
          <!-- 悬浮信息：左对齐四段（标题/副标题/分隔横线/描述），逐行进出场——
               换流时旧行向上依次掠出、新行依次补入。z-index 取 dropline：换流
               瞬间侧卡会掠过中央，文字留在 content 层会被卡面盖住 -->
          <div class="live-info">
            <h2
              class="info-name"
              in:fly={{ y: 16, duration: reducedMotion.value ? 0 : 260, delay: reducedMotion.value ? 0 : 100 }}
              out:fly={{ y: -14, duration: reducedMotion.value ? 0 : 130 }}
            >
              {stationName(currentEntry)}
            </h2>
            {#if currentEntry.subtitle}
              <p
                class="info-sub"
                in:fly={{ y: 16, duration: reducedMotion.value ? 0 : 260, delay: reducedMotion.value ? 0 : 170 }}
                out:fly={{ y: -14, duration: reducedMotion.value ? 0 : 130, delay: reducedMotion.value ? 0 : 45 }}
              >
                {currentEntry.subtitle}
              </p>
            {/if}
            <span
              class="info-rule"
              in:fly={{ y: 10, duration: reducedMotion.value ? 0 : 220, delay: reducedMotion.value ? 0 : 210 }}
              out:fly={{ y: -10, duration: reducedMotion.value ? 0 : 110, delay: reducedMotion.value ? 0 : 70 }}
            ></span>
            {#if currentEntry.description}
              <p
                class="info-desc"
                in:fly={{ y: 14, duration: reducedMotion.value ? 0 : 240, delay: reducedMotion.value ? 0 : 240 }}
                out:fly={{ y: -12, duration: reducedMotion.value ? 0 : 120, delay: reducedMotion.value ? 0 : 90 }}
              >
                {currentEntry.description}
              </p>
            {/if}
            {#if playingUrl === currentUrl}
              <p
                class="info-status"
                in:fly={{ y: 14, duration: reducedMotion.value ? 0 : 240, delay: reducedMotion.value ? 0 : 300 }}
                out:fly={{ y: -12, duration: reducedMotion.value ? 0 : 120, delay: reducedMotion.value ? 0 : 120 }}
              >
                <span class="stage-badge" class:on={phase.tone === 'on'} class:wait={phase.tone === 'wait'}>
                  <span class="stage-dot" aria-hidden="true"></span>{phase.text}
                </span>
                <span class="info-listened">{formatTime(listened)}</span>
              </p>
            {/if}
          </div>
        {/key}
      {/if}

      <!-- 开播引导：焦点卡外侧下方，悬停浮现。焦点是加号卡时不出现（那一步
           是添加，不是开播），直播中由信息块的徽章+计时接管 -->
      {#if currentEntry && playingUrl !== currentEntry.url}
        <p class="deck-hint" aria-hidden="true">{t('live.clickToPlay')}</p>
      {/if}
    </div>
  </section>
  {/if}
</div>

{#if adding}
  <Dialog
    title={t('live.addStream')}
    autofocus={() => urlFieldRef?.focus()}
    onclose={closeAdd}
  >
    <div class="add-form">
      <Field
        label={t('live.fieldUrl')}
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
        label={t('live.fieldName')}
        value={draftName}
        maxlength={80}
        placeholder={t('live.fieldNamePh')}
        oninput={(value) => (draftName = value)}
        onenter={submitAdd}
      />
      <Field
        label={t('live.fieldSubtitle')}
        value={draftSubtitle}
        maxlength={60}
        placeholder={t('live.fieldSubtitlePh')}
        oninput={(value) => (draftSubtitle = value)}
        onenter={submitAdd}
      />
      <Field
        label={t('live.fieldDesc')}
        value={draftDescription}
        maxlength={200}
        placeholder={t('live.fieldDescPh')}
        oninput={(value) => (draftDescription = value)}
        onenter={submitAdd}
      />
    </div>
    {#snippet actions()}
      <Button onclick={closeAdd}>{t('common.cancel')}</Button>
      <Button variant="primary" disabled={!draftUrl.trim()} onclick={submitAdd}>{t('live.addConfirm')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .live-tabs { display: flex; justify-content: center; margin-bottom: 8px; }
  .live {
    --live-card-w: 250px;
    --live-card-h: 400px;

    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 22px 8px;
  }

  /* 内容区高度吃紧时卡组缩一档，头部与信息块不动 */
  @media (max-height: 799px) {
    .live {
      --live-card-w: 200px;
      --live-card-h: 320px;
    }
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

  /* perspective 自成堆叠上下文：卡的层级只在舞台内比较（slotZ 给的数字），
     与 --z-* 阶梯无关；箭头/信息块仍在阶梯上 */
  .stage {
    position: relative;
    width: calc(var(--live-card-w) * 3.2);
    height: var(--live-card-h);
    perspective: 1000px;
  }

  .live-card {
    --card-tx: 0px;
    --card-rot: 0deg;
    --card-sc: 1;

    position: absolute;
    left: 50%;
    top: 50%;
    width: var(--live-card-w);
    height: var(--live-card-h);
    transform: translate(-50%, -50%) translateX(var(--card-tx)) rotateY(var(--card-rot))
      scale(var(--card-sc));
    transition:
      transform var(--live-swap-ms, 800ms) var(--ease),
      opacity var(--live-swap-ms, 800ms) var(--ease);
  }

  /* 焦点卡：居中放大，倾斜角由悬停写入 */
  .live-card.pos-current {
    --card-sc: 1.12;
    --card-rot: var(--live-tilt, 0deg);
  }

  /* 层叠位：向内翻（内缘朝向焦点），深度1就是原版单张侧卡的视觉
     （±1.1w / ∓25deg / 0.9），更深的卡在它身后继续向外退、继续收小 */
  .live-card.side-right.depth-1 {
    --card-tx: calc(var(--live-card-w) * 1.1);
    --card-rot: -25deg;
    --card-sc: 0.9;
  }

  .live-card.side-right.depth-2 {
    --card-tx: calc(var(--live-card-w) * 1.26);
    --card-rot: -28deg;
    --card-sc: 0.8;
    opacity: 0.75;
  }

  .live-card.side-right.depth-3 {
    --card-tx: calc(var(--live-card-w) * 1.38);
    --card-rot: -30deg;
    --card-sc: 0.72;
    opacity: 0.4;
  }

  .live-card.side-left.depth-1 {
    --card-tx: calc(var(--live-card-w) * -1.1);
    --card-rot: 25deg;
    --card-sc: 0.9;
  }

  .live-card.side-left.depth-2 {
    --card-tx: calc(var(--live-card-w) * -1.26);
    --card-rot: 28deg;
    --card-sc: 0.8;
    opacity: 0.75;
  }

  .live-card.side-left.depth-3 {
    --card-tx: calc(var(--live-card-w) * -1.38);
    --card-rot: 30deg;
    --card-sc: 0.72;
    opacity: 0.4;
  }

  /* 超出可见深度：停在边界槽位的transform上整体隐去（折返时从原位长回来） */
  .live-card.depth-out {
    opacity: 0;
    pointer-events: none;
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
    background: color-mix(in srgb, var(--surface-3) 45%, transparent);
    color: var(--text-1);
    cursor: pointer;
    overflow: hidden;
  }

  .face-mark {
    color: color-mix(in srgb, var(--accent) 46%, transparent);
  }

  /* 封面：有就铺满卡面（本地路径暂不支持，只收 http(s)，见字段文档） */
  .face-cover {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .card-remove {
    position: absolute;
    top: 8px;
    right: 8px;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
    z-index: var(--z-stage);
  }

  /* 删除按钮的对面（左上角）放一枚纯装饰的 radio：hover 时与删除键同时
     浮现，左右对称。不可点——纯视觉押韵 */
  .card-mark {
    position: absolute;
    top: 13px;
    left: 13px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    color: var(--text-3);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
    z-index: var(--z-stage);
  }

  .live-card:hover .card-remove,
  .card-remove:focus-visible,
  .live-card:hover .card-mark {
    opacity: 1;
  }

  /* 队尾加号卡：它是真按钮（点=开新增弹窗或转进来），复位 button 默认样式
     （壳上的定位/变换都由 .live-card 提供）；虚线边区别于"实在的流" */
  .live-card.add-card {
    appearance: none;
    border: none;
    padding: 0;
    background: none;
    font: inherit;
    cursor: pointer;
  }

  .add-face {
    border-style: dashed;
    border-color: var(--line-strong);
    color: var(--text-3);
  }

  .add-card:hover .add-face,
  .add-card:focus-visible .add-face {
    border-color: color-mix(in srgb, var(--accent) 55%, transparent);
    color: var(--text-2);
  }

  .add-text {
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

  /* 切换按钮放大一档（lg 是 36px，轮播是本页唯一操作，值得这个体量）；
     IconButton 的尺寸档是组件 scoped 的，这里借 :global 覆盖 */
  .deck-arrow :global(.k-icon-btn) {
    width: 52px;
    height: 52px;
    border-radius: var(--radius-lg);
  }

  .stage.locked .deck-arrow {
    opacity: 0.5;
    pointer-events: none;
  }

  /* 信息块：贴着中央卡左下沿，文字压在卡面上（指针事件让给卡）。
     --z-stage(40)：卡的层内 z 由 slotZ 给数字（焦点 30、侧卡递减），信息块
     必须压过**所有**卡——阶梯里 dock 与 modal 之间这一档就是舞台浮层位。
     文本全部左对齐，顺序：标题 / 副标题 / 分隔横线 / 描述（/ 状态） */
  .live-info {
    position: absolute;
    left: calc(var(--live-card-w) / 1.5);
    bottom: calc(var(--live-card-h) / 8);
    display: grid;
    justify-items: start;
    gap: 2px;
    max-width: calc(var(--live-card-w) * 1.4);
    text-align: left;
    pointer-events: none;
    z-index: var(--z-stage);
  }

  /* 开播引导：焦点卡**外侧下方**（舞台下缘之外）。焦点卡有 1.12 倍缩放，
     底边溢出舞台约 24px——10px 的空隙会压到卡上，36px 才真正离开卡体。
     焦点是加号卡时不出现 */
  .deck-hint {
    position: absolute;
    top: calc(100% + 36px);
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    font-size: var(--fs-sm);
    letter-spacing: 0.04em;
    color: var(--text-3);
    opacity: 0;
    transition: opacity var(--dur-base) var(--ease);
    pointer-events: none;
    z-index: var(--z-content);
  }

  .deck:hover .deck-hint {
    opacity: 1;
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

  .info-sub {
    margin: 0;
    overflow: hidden;
    font-size: var(--fs-sm);
    letter-spacing: 0.04em;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
  }

  /* 分隔横线块：标题组与描述之间的短横线 */
  .info-rule {
    width: 44px;
    height: 2px;
    margin: 6px 0;
    background: var(--line-strong);
  }

  .info-desc {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    margin: 0;
    overflow: hidden;
    font-size: var(--fs-xs);
    line-height: 1.5;
    color: var(--text-3);
    overflow-wrap: anywhere;
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

  /* 模态框表单 */

  .add-form {
    display: grid;
    gap: 12px;
    padding: 14px 16px;
  }

  @media (prefers-reduced-motion: reduce) {
    .live-card,
    .deck-arrow {
      transition-duration: 0s;
    }
  }
</style>
