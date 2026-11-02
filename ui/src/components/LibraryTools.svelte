<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, RefreshCw, ScanSearch } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';
  import Progress from './ui/Progress.svelte';
  import Icon from './Icon.svelte';
  import { errorMessage } from '../lib/errors';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { notify } from '../stores/notify.svelte';
  import { scan } from '../stores/scan.svelte';

  /**
   * 曲库组内容：单张卡片承载目录条目、动作、报错与执行状态。
   *
   * 宿主为设置页的「曲库」组。本组件不依赖页面壳，只派发动作并读取
   * `library` / `scan` 两个 store，因此更换宿主不影响逻辑。
   *
   * 目录条目列表：每行一条路径，右侧为删除按钮，删除先出确认条（可连曲目
   * 一并清除或仅停止监听）。路径以整行条目增删，不使用可编辑文本框。
   *
   * 执行状态（分隔线以下）：进行中显示"⟳ 曲库索引 · 阶段"、进度条与数字明细，
   * 取消按钮位于同一行右端（全应用唯一的取消入口；其他页面的进度条仅作环境
   * 指示）。结束后显示上次索引的统计。自动刷新为静默管线，不在此占版面。
   * 扫描报错内置于卡片（`scan.lastError`），不弹全局 toast。
   *
   * 数据流：目录列表来自 `library.roots`（后端归一化后回填），本组件只派发
   * 动作；曲目删除后统一走 `scan.afterScan()` 收尾，使列表、统计、歌单同步刷新。
   */
  let saving = $state(false);
  /** 目录卡片的内置报错（保存/删除/统计中的失败），下次动作开始时清除 */
  let rootsError = $state<string | null>(null);

  /**
   * 移除目录的确认态。
   *
   * 需要确认的原因：直接改写 `library.roots` 会使该目录下的曲目永久留在库中
   * （后续扫描将其标记为 missing 而非删除）。确认条提供两种处理：连同曲目
   * 一并清除（按路径前缀删除索引记录，不触碰磁盘），或仅停止监听并保留曲目。
   */
  interface PendingRemoval {
    paths: string[];
    count: number | null;
  }
  let pendingRemoval = $state<PendingRemoval | null>(null);

  /** 清空曲库的确认态（删除全部曲目记录：歌单壳保留，条目/历史级联清空） */
  let clearConfirm = $state(false);
  let clearing = $state(false);

  /* 两个确认条是页面内联区块，不使用模态语义：无焦点圈禁与滚动锁的容器不应声明 role="alertdialog"；取消键也不设
     autofocus，非模态区块不夺取焦点。出现时的播报由条内 role="alert" 的文本承担。 */

  const stats = $derived(library.stats);

  onMount(() => {
    void library.loadRoots();
  });

  /* 目录条目操作 */

  /** 移除单个目录：先查影响面（曲目数），出确认条 */
  async function requestRemove(root: string): Promise<void> {
    rootsError = null;
    try {
      const count = await library.countForRemoval([root]);
      pendingRemoval = { paths: [root], count };
    } catch (err) {
      rootsError = errorMessage(err);
    }
  }

  /** 保存根列表。失败时在卡片内显示错误；列表以 store 为准，不保留本地草稿 */
  async function saveRoots(next: string[], toast: string): Promise<void> {
    saving = true;
    rootsError = null;
    try {
      await library.saveRoots(next);
      notify.showToast(toast);
    } catch (err) {
      rootsError = errorMessage(err);
    } finally {
      saving = false;
    }
  }

  /** 删除曲目并保存：前缀清除（外键级联清歌单/队列/历史）→ 存根列表 → 统一收尾 */
  async function confirmPrune(): Promise<void> {
    const p = pendingRemoval;
    if (!p) return;
    saving = true;
    rootsError = null;
    try {
      const pruned = await library.pruneRoots(p.paths);
      const lower = new Set(p.paths.map((x) => x.trim().toLowerCase()));
      const next = library.roots.filter((r) => !lower.has(r.trim().toLowerCase()));
      await library.saveRoots(next);
      pendingRemoval = null;
      notify.showToast(`已停止监听 ${p.paths.length} 个目录，清除 ${pruned} 首曲目`);
      saving = false;
      await scan.afterScan();
    } catch (err) {
      rootsError = errorMessage(err);
      saving = false;
    }
  }

  /** 仅停止监听（保留曲目，下轮扫描它们会被标离线，不删） */
  async function confirmKeep(): Promise<void> {
    const p = pendingRemoval;
    if (!p) return;
    const lower = new Set(p.paths.map((x) => x.trim().toLowerCase()));
    pendingRemoval = null;
    await saveRoots(
      library.roots.filter((r) => !lower.has(r.trim().toLowerCase())),
      `已停止监听 ${p.paths.length} 个目录（曲目保留在库中）`,
    );
  }

  /** 放弃本次删除 */
  function dismissRemoval(): void {
    pendingRemoval = null;
  }

  /** 添加目录：系统选择器选中即追加（后端 `library_choose_root` 追加语义） */
  async function addRoot(): Promise<void> {
    await scan.pickRootAndScan();
  }

  /* 清空曲库 */

  async function doClear(): Promise<void> {
    clearing = true;
    rootsError = null;
    try {
      const removed = await library.clearLibrary();
      clearConfirm = false;
      notify.showToast(`已清空曲库记录（${removed} 首），磁盘文件未动`);
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
    if (phase === 'done') return '已完成';
    if (phase === 'cancelled') return '已取消';
    return phase === 'walking' ? '遍历中' : phase === 'importing' ? '入库中' : '解析中';
  }
</script>

<div class="library-tools">
  <!-- 单卡片布局：动作按钮位于头部右侧，目录条目与执行状态同卡 -->
  <div class="library-roots">
    <div class="roots-head">
      <strong>曲库目录</strong>
      <div class="roots-actions">
        <Button size="sm" onclick={() => void addRoot()} disabled={saving || clearing || library.scanning}>
          {#snippet icon()}<FolderOpen size={14} />{/snippet}
          添加目录
        </Button>
        <Button variant="primary" size="sm" onclick={() => void scan.start()} disabled={library.scanning || clearing}>
          {#snippet icon()}<ScanSearch size={14} />{/snippet}
          {library.scanning ? '索引中…' : '立即索引'}
        </Button>
        {#if stats && stats.tracks > 0}
          <Button variant="danger" size="sm" onclick={() => (clearConfirm = true)} disabled={saving || clearing || library.scanning}>
            清空曲库
          </Button>
        {/if}
        <!-- 清空播放历史：与「清空曲库」同属清除一类记录的动作，排在同一行。
             不设确认——播放历史不承载其他数据，toast 即时反馈，与首页同语义 -->
        <Button size="sm" onclick={() => void history.clear()} disabled={clearing}>
          清空播放历史
        </Button>
      </div>
    </div>

    {#if rootsError}
      <div class="tool-error" role="alert">{rootsError}</div>
    {/if}

    {#if pendingRemoval}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <!-- Esc 处理只服务"焦点在确认条内"的场景（从危险/取消键冒泡上来），
           容器自身不可聚焦、非交互——role=group 只为可访问性命名 -->
      <div
        class="roots-confirm"
        role="group"
        aria-label="移除目录确认"
        onkeydown={(event) => {
          // 焦点在确认条内（危险/取消键）时 Esc 归它：stopPropagation 挡住
          // window 上的全局 Esc（否则关确认条的同时还会清掉列表选择集）
          if (event.key === 'Escape') {
            event.stopPropagation();
            dismissRemoval();
          }
        }}
      >
        <span class="roots-confirm-text" role="alert">
          将停止监听 <b>{pendingRemoval.paths[0]}</b>
          {#if pendingRemoval.count !== null}，涉及 <b>{pendingRemoval.count}</b> 首曲目{/if}。
        </span>
        <div class="roots-confirm-actions">
          <Button size="sm" variant="danger" onclick={() => void confirmPrune()} disabled={saving}>
            删除曲目并停止监听
          </Button>
          <Button size="sm" onclick={() => void confirmKeep()} disabled={saving}>
            仅停止监听
          </Button>
          <Button variant="link" onclick={dismissRemoval} disabled={saving}>取消</Button>
        </div>
      </div>
    {/if}

    {#if clearConfirm}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <!-- Esc 处理只服务"焦点在确认条内"的场景（从危险/取消键冒泡上来），
           容器自身不可聚焦、非交互——role=group 只为可访问性命名 -->
      <div
        class="roots-confirm"
        role="group"
        aria-label="清空曲库确认"
        onkeydown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            clearConfirm = false;
          }
        }}
      >
        <span class="roots-confirm-text" role="alert">
          将删除全部 <b>{stats?.tracks ?? 0}</b> 首曲目的索引记录，歌单内容、播放队列与播放历史一并清空
          （歌单本身保留；<b>磁盘文件不动</b>）。此操作不可撤销。
        </span>
        <div class="roots-confirm-actions">
          <Button size="sm" variant="danger" onclick={() => void doClear()} disabled={clearing}>
            确认清空
          </Button>
          <Button variant="link" onclick={() => (clearConfirm = false)} disabled={clearing}>取消</Button>
        </div>
      </div>
    {/if}

    {#if library.roots.length === 0}
      <div class="dim roots-empty">还没有目录。点「添加目录」选择音乐文件夹建立索引。</div>
    {:else}
      <ul class="root-list">
        {#each library.roots as root (root)}
          <li class="root-row">
            <span class="root-path" title={root}>{root}</span>
            <!-- beets 相关设置不在本组件：曲库页只管理应用自身的索引库，
                 beets 的 config.yaml 由「整理库」页独立维护 -->
            <IconButton
              label={`移除目录 ${root}`}
              size="sm"
              disabled={saving || clearing}
              onclick={() => void requestRemove(root)}
            >
              <Icon name="close" size={12} />
            </IconButton>
          </li>
        {/each}
      </ul>
    {/if}

    <!-- 执行状态并入本卡，以分隔线分区。进行中：阶段与任务名一行，取消按钮位于该行右端；结束后只留一行统计。不设标题行——分隔线已表达分区，「执行状态」四字不携带额外信息 -->
    <div class="status-block">
      {#if scan.lastError}
        <div class="tool-error" role="alert">{scan.lastError}</div>
      {/if}

      {#if library.scanning && library.scan}
        {@const progress = library.scan}
        <div class="status-line">
          <span class="task-name">
            <RefreshCw size={13} class="spinning" /> 曲库索引 · {phaseText(progress.phase)}
          </span>
          <Button variant="link" onclick={() => scan.cancel()}>取消任务</Button>
        </div>
        <Progress value={progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8} label="曲库索引进度" size="md" />
        <div class="task-detail">
          {#if progress.phase === 'walking'}
            正在遍历目录…
          {:else}
            {progress.scanned}/{progress.total ?? '?'} 已入库 · 解析 {progress.parsed}{#if progress.parsed > progress.scanned}<span class="dim">
              （排队 {progress.parsed - progress.scanned}）</span>{/if} · 新增 {progress.imported} · 更新 {progress.updated} · 跳过 {progress.skipped} · 失败 {progress.failed}
          {/if}
        </div>
      {:else if library.scan}
        {@const progress = library.scan}
        <div class="task-detail">
          {#if progress.phase === 'done'}
            上次索引：新增 {progress.imported} · 更新 {progress.updated} · 离线 {progress.missing} · 跳过 {progress.skipped}{#if progress.failed} · <span class="danger">失败 {progress.failed}</span>{/if} · {Math.round(progress.elapsedMs / 100) / 10} 秒
          {:else if progress.phase === 'cancelled'}
            上次索引已取消（已完成部分保留）
          {:else}
            上次索引异常退出（{progress.phase}）
          {/if}
        </div>
      {:else}
        <div class="task-detail dim">还没有索引记录。</div>
      {/if}
    </div>
  </div>
</div>
