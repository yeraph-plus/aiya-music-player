<script lang="ts">
  import type { PlayerState } from '../../lib/ipc/generated/PlayerState';
  import { artworkUrl } from '../../lib/artwork';
  import {
    AUDIO_RATE_MAX,
    AUDIO_RATE_MIN,
    AUDIO_RATE_STEP,
    audioBadge,
    audioTooltip,
    clampRate,
    formatAudioSpec,
    formatRate,
    isHiRes,
  } from '../../lib/audio-format';
  import { formatTime, repeatLabel } from '../../lib/format';
  import { overlay } from '../../stores/overlay.svelte';
  import { ChevronUp, SlidersHorizontal } from '@lucide/svelte';
  import Icon from '../Icon.svelte';
  import Cover from '../ui/Cover.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Popover from '../ui/Popover.svelte';
  import Slider from '../ui/Slider.svelte';

  /**
   * 底部播放条：**唯一常驻的播放控制面**。
   *
   * B3b-2 换皮时改了三处**实现**（外观口径不变）：
   * 1. 两个 `<input type="range">` → `Slider`（bits-ui）。顺带把全局
   *    `input[type=range]` 那套 `-webkit-slider-thumb` 伪元素样式删了 ——
   *    它在亮色主题下拇指是深色圆点、深色主题下又是浅色，两套值各写一遍。
   *    **进度用 `oncommit`、音量用 `oninput`**：拖动中每帧 seek 会让 Rust 侧
   *    反复 flush 输出缓冲（实测有咔哒声），音量则相反 —— 用户期待拖的时候就听到。
   * 2. 图标键 → `IconButton`（圆形 + `solid` 只给中间那颗播放键）。
   * 3. 封面 → `Cover size="md"`（48px）。
   *
   * docs/25 §6 按键重排（用户五点反馈）：
   * - 左侧歌曲区域**本身就是正在播放页的入口**（侧栏席位已撤）；
   * - 主五键 = 加入歌单 / 上一首 / 播放 / 下一首 / 模式单键循环；
   * - 队列与 EQ 改走窗口右缘的 `SideDrawer`（AppShell 宿主），这里只留
   *   触发键——气泡定位抖动的根源（floating-ui 测量+翻转）整个消失；
   * - 倍速保持 Popover 气泡（小面板贴键，无抖动问题），迁到右侧键区。
   */

  interface Props {
    player: PlayerState | null;
    queueOpen: boolean;
    eqOpen: boolean;
    /** EQ 是否启用（启用时触发键打点，不打开抽屉也能看出状态） */
    eqEnabled: boolean;
    /** 倍速滑杆的本地草稿（player store 防抖对账，见 dragRate）；null = 跟随后端 */
    rateDraft: number | null;
    ontoggle: () => void;
    onnext: () => void;
    onprev: () => void;
    onseek: (ms: number) => void;
    onvolume: (value: number) => void;
    onmute: () => void;
    onrate: (rate: number) => void;
    /** 播放模式单键循环（列表循环→单曲循环→随机→顺序） */
    onmode: () => void;
    /** 队列抽屉开合（ui store 唯一入口；SideDrawer 宿主在 AppShell） */
    onqueueopen: (open: boolean) => void;
    /** EQ 抽屉开合（与队列互斥，ui store 负责清理） */
    oneqopen: (open: boolean) => void;
    /** 跳正在播放页 / 从正在播放页退回（左侧歌曲区域的点击目标，是个开关） */
    onshownowplaying: () => void;
    /** 正在播放页是否开着：决定封面角标折线箭头的朝向 */
    npOpen: boolean;
  }

  let {
    player,
    queueOpen,
    eqOpen,
    eqEnabled,
    rateDraft,
    ontoggle,
    onnext,
    onprev,
    onseek,
    onvolume,
    onmute,
    onrate,
    onmode,
    onqueueopen,
    oneqopen,
    onshownowplaying,
    npOpen,
  }: Props = $props();

  /**
   * 拖动中的进度镜像。
   *
   * 拖动时不能直接显示 `player.positionMs`：播放引擎每 ~250ms 推一帧位置，
   * 拿它渲染会让拇指在跟手与跳回之间来回。松手（`oncommit`）才 seek，
   * 之后 `dragging=false`，位置重新以后端为准。
   */
  let dragging = $state(false);
  let dragMs = $state(0);
  let rateOpen = $state(false);

  const positionMs = $derived(dragging ? dragMs : (player?.positionMs ?? 0));
  const durationMs = $derived(player?.durationMs ?? 0);
  /**
   * bits-ui Slider 的吸附坑（真机实测冻结进度条，mock 测不出来）：
   * value 不在 step 网格上时，SliderRootState 内部的 watch 会把它吸附到
   * 最近网格点——这次写入走的是 `onValueChange` 通道，于是**程序化的
   * 位置更新也会回调 oninput**，`dragging` 被卡成 true，`positionMs`
   * 从此钉死在 dragMs（引擎继续报位置，但拖动镜像把它全盖住）。
   * 播放位置的毫秒值对 step=100 几乎总是非法 → 必现；mock 后端的位置
   * 定时器同样按 100ms 步进推进（见 mock/backend.ts 的 tickPosition），
   * 恰好永远合法 → e2e 全绿。解法：传入前自己吸附到网格，内部
   * watch 看到的永远是合法值，不会触发那次写入。
   */
  const sliderMs = $derived(Math.round(positionMs / 100) * 100);
  const playing = $derived(player?.status === 'playing');
  const loading = $derived(player?.status === 'loading');
  const hasTrack = $derived(Boolean(player?.current));

  // 滑杆防抖期间显示本地草稿（即时跟手），草稿作废后回落到后端状态
  const rate = $derived(clampRate(rateDraft ?? (player?.rate ?? 1)));
  const audio = $derived(player?.current?.audio ?? null);
  const badge = $derived(audioBadge(audio?.codec ?? null, audio?.container ?? null));
  const spec = $derived(formatAudioSpec(audio));
  const tooltip = $derived(audioTooltip(audio));

  const volume = $derived(player?.muted ? 0 : (player?.volume ?? 0.85));

  /**
   * 播放模式（单键循环的四态）。后端字段是 repeat + shuffle 两个正交开关，
   * 这里合成用户心智里的一个"模式"：随机优先显示（shuffle 开着就是随机，
   * 不管 repeat 是什么）。
   */
  type PlayMode = 'all' | 'one' | 'shuffle' | 'sequential';
  const mode = $derived<PlayMode>(
    player?.shuffle ? 'shuffle' : (player?.repeat ?? 'off') === 'one' ? 'one' : (player?.repeat ?? 'off') === 'all' ? 'all' : 'sequential',
  );
  const modeLabel = $derived(
    mode === 'shuffle' ? '随机播放' : repeatLabel(mode === 'sequential' ? 'off' : mode),
  );
  /** PlayMode → Icon 语义名（all 用 repeat 图标，one 用 repeatOne） */
  const modeIcon = $derived(
    mode === 'all' ? 'repeat' : mode === 'one' ? 'repeatOne' : mode,
  );

  /** 当前曲目加入歌单：复用全局 addToPlaylist 对话框（OverlayHost 渲染） */
  function addCurrentToPlaylist(): void {
    const id = player?.current?.id;
    if (id) overlay.dialog = { kind: 'addToPlaylist', ids: [id] };
  }
</script>

<footer class="player">
  <!--
    歌曲区域 = 正在播放页**开关**（docs/25 §6：侧栏席位撤了，入口搬到这里——
    "正在听什么"的信息本身就在这，点它是最近的路）。无曲目也允许点进
    空态页（页面自己会说明"未在播放"），不做 disabled。

    封面上的折线箭头 2026-10-30 改成 **hover 提示**：原来是压在封面右下角的
    16px 小徽章，静止时也在，等于把封面裁掉一个角、又小到看不清；现在整块
    封面盖一层暗色蒙版 + 居中折线箭头，只在悬停/键盘聚焦时出现——朝上 =
    可以展开，朝下 = 已经展开、再点收起。页内不再放返回键，退出走这里或顶栏返回。
  -->
  <button
    class="now"
    class:open={npOpen}
    onclick={onshownowplaying}
    title={npOpen ? '收起正在播放' : '打开正在播放'}
    aria-label={npOpen ? '收起正在播放页' : '打开正在播放页'}
    aria-expanded={npOpen}
  >
    <span class="now-cover">
      <Cover
        src={player?.current?.artworkId ? artworkUrl(player.current.artworkId, 96) : null}
        size="md"
      />
      <span class="now-cue" aria-hidden="true">
        <span class="now-cue-icon">
          <ChevronUp size={20} strokeWidth={2.4} />
        </span>
      </span>
    </span>

    <div class="meta">
      <div class="title" title={player?.current?.title ?? ''}>
        {player?.current?.title ?? '未在播放'}
      </div>
      <div class="line2">
        {#if player?.current}
          <span class="artist">{player.current.artist ?? '未知艺人'}</span>
          {#if badge}
            <span
              class="fmt-badge"
              class:hires={audio ? isHiRes(audio) : false}
              title={tooltip ?? ''}
            >
              {badge}
            </span>
          {/if}
          {#if spec}
            <span class="fmt-spec" title={tooltip ?? ''}>{spec}</span>
          {/if}
        {:else}
          选择一首歌开始
        {/if}
      </div>
    </div>
  </button>

  <div class="transport">
    <div class="controls">
      <IconButton
        label="把当前曲目加入播放列表"
        round
        disabled={!hasTrack}
        onclick={addCurrentToPlaylist}
      >
        <Icon name="heart" />
      </IconButton>

      <IconButton label="上一首" round disabled={!hasTrack} onclick={onprev}>
        <Icon name="prev" size={18} />
      </IconButton>

      <IconButton
        label={playing ? '暂停' : '播放'}
        size="lg"
        variant="solid"
        round
        disabled={!hasTrack && (player?.queueLength ?? 0) === 0}
        onclick={ontoggle}
      >
        {#if loading}
          <span class="spinner"></span>
        {:else if playing}
          <Icon name="pause" size={16} />
        {:else}
          <Icon name="play" size={16} />
        {/if}
      </IconButton>

      <IconButton label="下一首" round disabled={!hasTrack} onclick={onnext}>
        <Icon name="next" size={18} />
      </IconButton>

      <!-- 模式单键循环：列表循环 → 单曲循环 → 随机 → 顺序 -->
      <IconButton label={`播放模式：${modeLabel}`} round pressed={mode !== 'sequential'} onclick={onmode}>
        <Icon name={modeIcon} />
      </IconButton>
    </div>

    <div class="timeline">
      <span class="t">{formatTime(positionMs)}</span>
      <Slider
        label="播放进度"
        value={sliderMs}
        min={0}
        max={durationMs || 1}
        step={100}
        disabled={!hasTrack || durationMs === 0}
        oninput={(value) => {
          dragging = true;
          dragMs = value;
        }}
        oncommit={(value) => {
          if (!dragging) return;
          dragging = false;
          onseek(value);
        }}
      />
      <span class="t">{formatTime(durationMs)}</span>
    </div>
  </div>

  <div class="right">
    <!--
      队列 / EQ 抽屉触发键（docs/25 §6）：内容宿主是 AppShell 的 SideDrawer
      （窗口右缘滑出，几何确定不抖）。这里只是普通开关键：aria-expanded
      手动同步，外点/Esc/焦点还原全部由抽屉负责。不用 IconButton：
      EQ 的"生效中"打点需要 relative 定位锚点，裸 button 自持样式。
    -->
    <button
      class="drawer-btn"
      aria-expanded={queueOpen}
      aria-label={`播放队列（${player?.queueLength ?? 0} 首）`}
      title={`播放队列（${player?.queueLength ?? 0} 首）`}
      onclick={() => onqueueopen(!queueOpen)}
    >
      <Icon name="queue" />
    </button>
    <button
      class="drawer-btn"
      class:on={eqEnabled}
      aria-expanded={eqOpen}
      aria-label={`均衡器（${eqEnabled ? '已启用' : '已关闭'}）`}
      title={`均衡器（${eqEnabled ? '已启用' : '已关闭'}）`}
      onclick={() => oneqopen(!eqOpen)}
    >
      <SlidersHorizontal size={16} />
      {#if eqEnabled}<span class="drawer-dot" aria-hidden="true"></span>{/if}
    </button>

    <!-- 倍速：保持气泡（小面板贴键无抖动问题），不进抽屉 -->
    <Popover
      open={rateOpen}
      onopenchange={(value) => (rateOpen = value)}
      side="top"
      align="end"
      sideOffset={12}
    >
      {#snippet trigger(props)}
        <button
          {...props}
          class={`rate-chip${rate !== 1 ? ' on' : ''}`}
          ondblclick={() => onrate(1)}
          aria-label="播放速度"
          title={`播放速度 ${formatRate(rate)}（双击复位 1.0×）`}
        >
          {formatRate(rate)}
        </button>
      {/snippet}

      <div class="rate-panel">
        <div class="rate-head">
          <span>播放速度</span>
          <button class="rate-reset" onclick={() => onrate(1)}>复位</button>
        </div>
        <Slider
          label="播放速度"
          value={rate}
          min={AUDIO_RATE_MIN}
          max={AUDIO_RATE_MAX}
          step={AUDIO_RATE_STEP}
          oninput={(value) => onrate(value)}
          width="100%"
        />
        <div class="rate-scale">
          <span>{formatRate(AUDIO_RATE_MIN)}</span>
          <span>1.0×</span>
          <span>{formatRate(AUDIO_RATE_MAX)}</span>
        </div>
      </div>
    </Popover>

    <IconButton label={player?.muted ? '取消静音' : '静音'} round onclick={onmute}>
      <Icon name={player?.muted || (player?.volume ?? 0) === 0 ? 'mute' : 'volume'} />
    </IconButton>
    <div class="volume">
      <Slider
        label="音量"
        value={volume}
        min={0}
        max={1}
        step={0.01}
        oninput={(value) => onvolume(value)}
        width="100%"
      />
    </div>
  </div>
</footer>

<style>
  /* ---------------- 整条 ---------------- */

  .player {
    display: grid;
    /* 左中右三段：左侧曲目信息固定下限、中间传输区占大头、右侧控制固定下限。
       三段的 minmax 让窄窗时先压缩左右，传输区不会被挤没。 */
    grid-template-columns: minmax(170px, 1fr) minmax(300px, 2.1fr) minmax(170px, 1fr);
    align-items: center;
    gap: 16px;
    height: var(--player-h);
    padding: 0 18px;
    /* 边线已撤（用户拍板"区域之间不留 border"）：与主区的分隔改由一条
     * **上抛阴影**承担——播放条是全应用最"实"的一层，阴影既给出分界，
     * 又暗示它悬浮在背景之上（方向朝上，光从上面来）。 */
    box-shadow: 0 -6px 18px color-mix(in srgb, var(--shadow) 50%, transparent);
    /* 半透明（docs/25 T4g 定档 95%——与顶栏同档，接近不透明）：
     * 全应用背景从底下透出来。底色用 surface-3（顶栏是 surface-2）——
     * 同档不同阶，有一点色差。原 backdrop-filter 摘除——持久壳层垫着
     * 动画频谱 canvas 就是每帧全条重模糊（docs/25 §5 方案 B 红线）。 */
    background: color-mix(in srgb, var(--surface-3) 95%, transparent);
  }

  /* ---------------- 左：当前曲目（= 正在播放页入口） ---------------- */

  .now {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 4px 6px;
    margin: -4px -6px; /* 抵消内距：视觉占位与原 div 一致，只多出悬停热区 */
    border-radius: 10px;
    text-align: left;
    transition: background var(--dur-fast) var(--ease);
  }

  .now:hover {
    background: var(--surface-hover);
  }

  /* 封面外面只多一层定位盒，供 hover 提示蒙版精确铺满封面 */
  .now-cover {
    position: relative;
    flex: none;
    display: block;
    border-radius: var(--radius-md);
  }

  /*
   * 折线箭头 = 「还有一页」的提示，同时是开合状态的读数：
   * 朝上（未展开，可以上去）/ 朝下（已展开，再点收起）。
   *
   * 形态是**盖住整块封面的暗色蒙版 + 居中箭头**（2026-10-30 用户拍板）：
   * 原来是压角的 16px 小徽章，静止时也在，等于从封面裁掉一个角还看不清。
   * 蒙版只在 hover / 键盘聚焦时淡入——48px 的封面经不起常驻装饰，
   * 而"这块能不能点"本来就是要伸手的时候才需要回答的问题。
   * 黑色蒙版 + 白箭头：落在任何封面上都是同一对比度，不跟主题走。
   */
  .now-cue {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    border-radius: var(--radius-md);
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--dur-fast) var(--ease);
  }

  .now:hover .now-cue,
  .now:focus-visible .now-cue {
    opacity: 1;
  }

  /* 箭头朝"将要去的那一侧"偏 1.5px：朝上 = 这页在上面。
     图标包一层自己的 span，而不是选 `.now-cue svg`——svg 是 ChevronUp
     组件渲染的，Svelte 的作用域规则选不到它（check 会报未使用选择器）。 */
  .now-cue-icon {
    display: grid;
    place-items: center;
    /* 箭头压在亮封面上也要立得住：极轻的一圈投影就够了，多了显脏 */
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.55));
    transition: transform var(--dur-fast) var(--ease);
  }

  .now:hover .now-cue-icon,
  .now:focus-visible .now-cue-icon {
    transform: translateY(-1.5px);
  }

  /* 已展开 = 箭头朝下（"按下去收起"）。旋转在前、位移在后，
     位移跟着进旋转后的坐标系 → 展开时箭头向下偏，和朝向一致 */
  .now.open .now-cue-icon {
    transform: rotate(180deg);
  }

  .now.open:hover .now-cue-icon,
  .now.open:focus-visible .now-cue-icon {
    transform: rotate(180deg) translateY(-1.5px);
  }

  .now:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .meta {
    min-width: 0;
    /* container-type 会让内容尺寸被抑制（contain: inline-size），
       flex 里 flex-basis:auto 就成了 0 —— 必须显式 flex:1 占位，
       否则 .title（自身 overflow:hidden）宽度 0 被整行裁掉 */
    flex: 1;
    /* 窄到放不下时隐藏规格文字，徽章保留（见下面 @container） */
    container-type: inline-size;
  }

  .title {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
    font-size: 11px;
    color: var(--text-2);
    overflow: hidden;
    white-space: nowrap;
  }

  .artist {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .fmt-badge {
    flex: none;
    padding: 0 4px;
    border-radius: 3px;
    border: 1px solid var(--line-strong);
    font-size: 9.5px;
    line-height: 14px;
    letter-spacing: 0.04em;
    color: var(--text-2);
  }

  .fmt-badge.hires {
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .fmt-spec {
    flex: none;
    font-size: 10px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  @container (max-width: 250px) {
    .fmt-spec {
      display: none;
    }
  }

  /* ---------------- 中：传输 + 进度 ---------------- */

  .transport {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--play-fg) 30%, transparent);
    border-top-color: var(--play-fg);
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .timeline {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 10.5px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .timeline .t {
    flex: none;
    width: 38px;
  }

  .timeline .t:last-child {
    text-align: right;
  }

  /* ---------------- 右：队列 / EQ / 倍速 / 音量 ---------------- */

  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    min-width: 0;
  }

  .volume {
    flex: none;
    width: 92px;
  }

  /* ---------------- 倍速面板（Popover 内容） ---------------- */

  /* 抽屉触发键（队列/EQ 共用）：内容宿主在 AppShell 的 SideDrawer，这里只是
     开关键。30px 圆形 ghost 键外观（与 IconButton 同款，但要挂 aria-expanded
     和 EQ 打点的 relative 锚点，裸 button 自持样式）。 */
  .drawer-btn {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    color: var(--text-2);
    flex: none;
    transition:
      background var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }

  .drawer-btn:hover {
    background: var(--surface-hover);
    color: var(--text-1);
  }

  .drawer-btn[aria-expanded='true'] {
    background: var(--accent-weak);
    color: var(--accent-text);
  }

  /* EQ 启用打点：不用 accent 底（那是"抽屉开着"），只点一颗小点表示"生效中" */
  .drawer-btn.on {
    color: var(--text-1);
  }

  .drawer-dot {
    position: absolute;
    right: 5px;
    bottom: 5px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
  }

  .rate-chip {
    flex: none;
    height: 22px;
    min-width: 44px;
    padding: 0 7px;
    border-radius: var(--radius-pill);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    border: 1px solid transparent;
    display: grid;
    place-items: center;
  }

  .rate-chip:hover {
    color: var(--text-1);
    background: var(--surface-hover);
  }

  /* 非 1× 高亮：和模式键的开启态同一套语义 */
  .rate-chip.on {
    color: var(--accent);
    background: var(--accent-weak);
  }

  .rate-panel {
    width: 186px;
  }

  .rate-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11.5px;
    margin-bottom: 10px;
  }

  .rate-reset {
    font-size: 11px;
    color: var(--text-2);
    padding: 1px 6px;
    border-radius: 4px;
  }

  .rate-reset:hover {
    color: var(--text-1);
    background: var(--surface-hover);
  }

  .rate-scale {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    margin-top: 6px;
  }
</style>
