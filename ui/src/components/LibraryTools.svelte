<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, Radar, RefreshCw, ScanSearch } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import Popover from './ui/Popover.svelte';
  import Progress from './ui/Progress.svelte';
  import Icon from './Icon.svelte';
  import { errorMessage } from '../lib/errors';
  import { t } from '../lib/i18n.svelte';
  import { isNative } from '../lib/window-chrome';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { scan } from '../stores/scan.svelte';

  /**
   * 曲库组内容：单张卡片承载目录条目、动作、报错与执行状态。
   *
   * 宿主为设置页的「曲库」组。本组件不依赖页面壳，只派发动作并读取
   * `library` / `scan` 两个 store，因此更换宿主不影响逻辑。
   *
   * 目录条目行：路径在左；行尾两颗图标钮——雷达图标是监听开关（停用叠斜
   * 划线、行变暗，随时可恢复；停止监听只影响扫描范围，不清理数据），垃圾桶
   * 打开删除确认气泡（报影响面曲目数，确认才清除索引并从两组列表移除，
   * 磁盘文件不动）。
   *
   * 执行状态区（分隔线以下）同时是本卡唯一的提示区：报错、动作结果反馈、
   * 清空曲库确认与扫描进度都渲染在此，不弹出独立提示层。进行中显示
   * "⟳ 曲库索引 · 阶段"、进度条与数字明细，取消按钮位于同一行右端（全应用
   * 唯一的取消入口；其他页面的进度条仅作环境指示）；结束后显示上次索引的
   * 统计。自动刷新为静默管线，不在此占版面。
   *
   * 数据流：目录列表来自 `library.roots`（监听中）与 `library.pausedRoots`
   * （停用，后端归一化后回填），本组件只派发动作；曲目删除后统一走
   * `scan.afterScan()` 收尾，使列表、统计、歌单同步刷新。
   */

  let saving = $state(false);
  /** 目录卡片的内置报错（保存/删除/统计中的失败），下次动作开始时清除 */
  let rootsError = $state<string | null>(null);

  /**
   * 动作结果反馈行（替代全局 toast）：本卡内成功/完成类消息的唯一出口，
   * 数秒后自动消退，下一条动作结果会覆盖它。报错走 `rootsError` /
   * `scan.lastError`，不进这条。
   */
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | null = null;
  const NOTICE_TTL_MS = 6000;

  function showNotice(text: string): void {
    notice = text;
    if (noticeTimer !== null) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => {
      notice = null;
      noticeTimer = null;
    }, NOTICE_TTL_MS);
  }

  /**
   * 删除确认气泡的状态：`openRemoval` 记录气泡属于哪一行（一次只开一个），
   * `removalCount` 是气泡打开时异步取到的影响面曲目数（null = 统计中，确认
   * 键禁用）。代次号防串台：连开两行时，前一行慢返回的计数不得落到后一行。
   */
  let openRemoval = $state<string | null>(null);
  let removalCount = $state<number | null>(null);
  let removalGeneration = 0;

  /** 清空曲库的确认态（删除全部曲目记录：歌单壳保留，条目/历史级联清空） */
  let clearConfirm = $state(false);
  let clearing = $state(false);

  /* 清空确认条是页面内联区块，不使用模态语义：无焦点圈禁与滚动锁的容器不应声明 role="alertdialog"；取消键也不设
     autofocus，非模态区块不夺取焦点。出现时的播报由条内 role="alert" 的文本承担。 */

  const stats = $derived(library.stats);

  /** 行展示序：监听中在前、停用在后（后端对两组各自保序归一化） */
  const displayRoots = $derived([...library.roots, ...library.pausedRoots]);

  /** 监听判定走大小写不敏感比较：后端归一化不改变已有条目的大小写 */
  function isWatched(root: string): boolean {
    const lower = root.trim().toLowerCase();
    return library.roots.some((r) => r.trim().toLowerCase() === lower);
  }

  /** 大小写不敏感地剔除一个路径（两组列表都用它做移动/遗忘） */
  function withoutPath(paths: string[], target: string): string[] {
    const lower = target.trim().toLowerCase();
    return paths.filter((p) => p.trim().toLowerCase() !== lower);
  }

  onMount(() => {
    void library.loadRoots();
    return () => {
      if (noticeTimer !== null) clearTimeout(noticeTimer);
    };
  });

  /* 目录条目操作 */

  /**
   * 开/关某目录的监听：在监听组与停用组之间移动，曲目不动。
   * 停用后其曲目在下轮索引被标记离线；恢复后下次索引重新纳入。
   * 失败时 store 不变，图标随状态弹回原位。
   */
  async function setWatched(root: string, watched: boolean): Promise<void> {
    saving = true;
    rootsError = null;
    try {
      if (watched) {
        await library.saveRoots(
          [...library.roots, root],
          withoutPath(library.pausedRoots, root),
        );
        showNotice(t('library.tools.noticeWatchResumed'));
      } else {
        await library.saveRoots(
          withoutPath(library.roots, root),
          [...library.pausedRoots, root],
        );
        showNotice(t('library.tools.noticeWatchPaused'));
      }
    } catch (err) {
      rootsError = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  /** 打开删除气泡时取影响面（曲目数）；失败收回气泡，在提示区报错 */
  async function fetchRemovalCount(root: string): Promise<void> {
    const generation = (removalGeneration += 1);
    removalCount = null;
    try {
      const count = await library.countForRemoval([root]);
      if (generation !== removalGeneration) return;
      removalCount = count;
    } catch (err) {
      if (generation !== removalGeneration) return;
      openRemoval = null;
      rootsError = errorMessage(err);
    }
  }

  /**
   * 气泡确认键：清除该目录曲目索引（前缀清除，外键级联清歌单/队列/历史，
   * 不触碰磁盘）并从两组列表移除——彻底遗忘这个目录。
   */
  async function confirmRemove(root: string): Promise<void> {
    saving = true;
    rootsError = null;
    try {
      const removed = await library.pruneRoots([root]);
      await library.saveRoots(
        withoutPath(library.roots, root),
        withoutPath(library.pausedRoots, root),
      );
      openRemoval = null;
      removalCount = null;
      showNotice(
        removed > 0
          ? t('library.tools.noticeRemoved', { n: removed })
          : t('library.tools.noticeRemovedNoTracks'),
      );
      await scan.afterScan();
    } catch (err) {
      rootsError = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  /** 添加目录：系统选择器选中即追加（后端 `library_choose_root` 追加语义） */
  async function addRoot(): Promise<void> {
    await scan.pickRootAndScan();
  }

  /**
   * Web 宿主没有系统目录选择器：绝对路径文本输入，追加进曲库根后立即索引。
   * 语义与桌面的选目录追加一致（`library.saveRoots` 全量写回 + `scan.start()`）；
   * 容器部署里宿主把任意目录挂进容器，路径在应用内配——输入容器内路径。
   */
  let rootDraft = $state('');
  const normPath = (p: string): string => p.trim().replace(/[\\/]+$/, '');
  async function addRootByPath(): Promise<void> {
    const path = normPath(rootDraft);
    if (!path || saving || clearing || library.scanning) return;
    if (displayRoots.some((r) => normPath(r).toLowerCase() === path.toLowerCase())) {
      showNotice(t('library.tools.noticeRootExists'));
      return;
    }
    saving = true;
    rootsError = null;
    try {
      await library.saveRoots([...library.roots, path], library.pausedRoots);
      rootDraft = '';
      await scan.start();
    } catch (err) {
      rootsError = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  /* 清空曲库 */

  async function doClear(): Promise<void> {
    clearing = true;
    rootsError = null;
    try {
      const removed = await library.clearLibrary();
      clearConfirm = false;
      showNotice(t('library.tools.noticeCleared', { n: removed }));
      clearing = false;
      await scan.afterScan();
    } catch (err) {
      rootsError = errorMessage(err);
      clearing = false;
    }
  }

  /* 执行状态卡片 */

  /**
   * 阶段文案（两段式进度）。
   *
   * 解析与入库两段在时间上重叠，只报单一百分比时，用户会在"解析快、入库慢"
   * 的区间看到进度条停滞。此处如实报告当前主活动，具体数字由下一行提供。
   */
  function phaseText(phase: string): string {
    if (phase === 'done') return t('library.tools.phaseDone');
    if (phase === 'cancelled') return t('library.tools.phaseCancelled');
    return phase === 'walking'
      ? t('library.tools.phaseWalking')
      : phase === 'importing'
        ? t('library.tools.phaseImporting')
        : t('library.tools.phaseParsing');
  }
</script>

<div class="library-tools">
  <!-- 单卡片布局：动作按钮位于头部右侧，目录条目与执行状态同卡 -->
  <div class="library-roots">
    <div class="roots-head">
      <strong>{t('library.tools.rootsTitle')}</strong>
      <div class="roots-actions">
        {#if isNative}
          <Button size="sm" onclick={() => void addRoot()} disabled={saving || clearing || library.scanning}>
            {#snippet icon()}<FolderOpen size={14} />{/snippet}
            {t('library.tools.addFolder')}
          </Button>
        {:else}
          <!-- Web 宿主：无系统选择器，绝对路径手输（容器部署把宿主目录挂进容器，
               路径在应用内配——AIYA_MUSIC_DIR 只是首启种子） -->
          <input
            class="root-input"
            type="text"
            bind:value={rootDraft}
            placeholder={t('library.tools.rootPlaceholder')}
            aria-label={t('library.tools.addFolder')}
            spellcheck="false"
            onkeydown={(e) => e.key === 'Enter' && void addRootByPath()}
          />
          <Button size="sm" onclick={() => void addRootByPath()} disabled={saving || clearing || library.scanning}>
            {#snippet icon()}<FolderOpen size={14} />{/snippet}
            {t('library.tools.addFolder')}
          </Button>
        {/if}
        <Button variant="primary" size="sm" onclick={() => void scan.start()} disabled={library.scanning || clearing}>
          {#snippet icon()}<ScanSearch size={14} />{/snippet}
          {library.scanning ? t('library.tools.indexing') : t('library.tools.indexNow')}
        </Button>
        {#if stats && stats.tracks > 0}
          <Button variant="danger" size="sm" onclick={() => (clearConfirm = true)} disabled={saving || clearing || library.scanning}>
            {t('library.tools.clearLibrary')}
          </Button>
        {/if}
        <!-- 清空播放历史：与「清空曲库」同属清除一类记录的动作，排在同一行。
             不设确认——播放历史不承载其他数据，提示区即时反馈，与首页同语义 -->
        <Button size="sm" onclick={() => void history.clear()} disabled={clearing}>
          {t('library.tools.clearHistory')}
        </Button>
      </div>
    </div>

    {#if displayRoots.length === 0}
      <div class="dim roots-empty">{t('library.tools.rootsEmpty')}</div>
    {:else}
      <ul class="root-list">
        {#each displayRoots as root (root)}
          {@const watched = isWatched(root)}
          <li class="root-row" class:unwatched={!watched}>
            <span class="root-path" title={root}>{root}</span>
            <!-- beets 相关设置不在本组件：曲库页只管理应用自身的索引库，
                 beets 的 config.yaml 由「整理库」页独立维护 -->
            <!-- 监听开关在删除键左侧：状态指示离路径近，破坏性动作（删除）居行尾。
                 雷达即状态——监听中正常显色，停用变暗并叠斜划线（lucide 无
                 radar-off，划线是同图标上的 CSS 覆盖层，天然成套） -->
            <button
              type="button"
              class="root-watch"
              class:off={!watched}
              aria-pressed={watched}
              aria-label={t('library.tools.watchAria', { path: root })}
              title={watched ? t('library.tools.watchOnTip') : t('library.tools.watchOffTip')}
              disabled={saving || clearing}
              onclick={() => void setWatched(root, !watched)}
            >
              <Radar size={14} />
            </button>

            <Popover
              open={openRemoval === root}
              onopenchange={(open) => {
                openRemoval = open ? root : null;
                if (open) void fetchRemovalCount(root);
                else removalCount = null;
              }}
              side="top"
              align="end"
            >
              {#snippet trigger(props)}
                <button
                  {...props}
                  type="button"
                  class="root-remove"
                  aria-label={t('library.tools.removeAria', { path: root })}
                  title={t('library.tools.removeTip')}
                  disabled={saving || clearing}
                >
                  <Icon name="trash" size={13} />
                </button>
              {/snippet}
              <!-- role="alert"：气泡出现即播报影响面；外点/Esc 关闭由 bits-ui 承担 -->
              <div class="roots-pop" role="alert">
                <span class="roots-pop-text">
                  {t('library.tools.removeBubblePrefix')} <b>{root}</b> {t('library.tools.removeBubbleTail')}
                  {#if removalCount === null}
                    {t('library.tools.removeBubbleCounting')}
                  {:else}
                    {t('library.tools.removeBubbleCountPrefix')} <b>{removalCount}</b> {t('library.tools.removeBubbleCountTail')}
                  {/if}
                </span>
                <div class="roots-pop-actions">
                  <Button
                    size="sm"
                    variant="danger"
                    disabled={saving || removalCount === null}
                    onclick={() => void confirmRemove(root)}
                  >
                    {t('library.tools.confirmRemove')}
                  </Button>
                  <Button variant="link" onclick={() => (openRemoval = null)}>{t('common.cancel')}</Button>
                </div>
              </div>
            </Popover>
          </li>
        {/each}
      </ul>
    {/if}

    <!-- 执行状态并入本卡，以分隔线分区，同时是本卡唯一提示区：报错 / 动作
         反馈 / 清空确认 / 扫描进度都渲染在此。进行中：阶段与任务名一行，取消
         按钮位于该行右端；结束后只留一行统计。不设标题行——分隔线已表达分区 -->
    <div class="status-block">
      {#if scan.lastError}
        <div class="tool-error" role="alert">{scan.lastError}</div>
      {/if}

      {#if rootsError}
        <div class="tool-error" role="alert">{rootsError}</div>
      {/if}

      {#if notice}
        <div class="tool-notice" role="status">{notice}</div>
      {/if}

      {#if clearConfirm}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <!-- Esc 处理只服务"焦点在确认条内"的场景（从危险/取消键冒泡上来），
             容器自身不可聚焦、非交互——role=group 只为可访问性命名 -->
        <div
          class="roots-confirm"
          role="group"
          aria-label={t('library.tools.clearConfirmLabel')}
          onkeydown={(event) => {
            if (event.key === 'Escape') {
              event.stopPropagation();
              clearConfirm = false;
            }
          }}
        >
          <span class="roots-confirm-text" role="alert">
            {t('library.tools.clearConfirmHead')} <b>{stats?.tracks ?? 0}</b> {t('library.tools.clearConfirmMid')}
            {t('library.tools.clearConfirmKept')}<b>{t('library.tools.clearConfirmFiles')}</b>{t('library.tools.clearConfirmTail')}
          </span>
          <div class="roots-confirm-actions">
            <Button size="sm" variant="danger" onclick={() => void doClear()} disabled={clearing}>
              {t('library.tools.confirmClear')}
            </Button>
            <Button variant="link" onclick={() => (clearConfirm = false)} disabled={clearing}>{t('common.cancel')}</Button>
          </div>
        </div>
      {/if}

      {#if library.scanning && library.scan}
        {@const progress = library.scan}
        <div class="status-line">
          <span class="task-name">
            <RefreshCw size={13} class="spinning" /> {t('library.tools.taskName')} · {phaseText(progress.phase)}
          </span>
          <Button variant="link" onclick={() => scan.cancel()}>{t('library.tools.cancelTask')}</Button>
        </div>
        <Progress value={progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8} label={t('library.tools.progressLabel')} size="md" />
        <div class="task-detail">
          {#if progress.phase === 'walking'}
            {t('library.tools.walkingLine')}
          {:else}
            {t('library.tools.progressMain', { scanned: progress.scanned, total: progress.total ?? '?', parsed: progress.parsed })}{#if progress.parsed > progress.scanned}<span class="dim">
              {t('library.tools.progressQueued', { n: progress.parsed - progress.scanned })}</span>{/if} {t('library.tools.progressTail', { imported: progress.imported, updated: progress.updated, skipped: progress.skipped, failed: progress.failed })}
          {/if}
        </div>
      {:else if library.scan}
        {@const progress = library.scan}
        <div class="task-detail">
          {#if progress.phase === 'done'}
            {t('library.tools.lastIndexMain', { imported: progress.imported, updated: progress.updated, missing: progress.missing, skipped: progress.skipped })}{#if progress.failed} · <span class="danger">{t('library.tools.lastIndexFailed', { failed: progress.failed })}</span>{/if} {t('library.tools.lastIndexDuration', { s: Math.round(progress.elapsedMs / 100) / 10 })}
          {:else if progress.phase === 'cancelled'}
            {t('library.tools.lastIndexCancelled')}
          {:else}
            {t('library.tools.lastIndexAborted', { phase: progress.phase })}
          {/if}
        </div>
      {:else}
        <div class="task-detail dim">{t('library.tools.noIndexRecord')}</div>
      {/if}
    </div>
  </div>
</div>
