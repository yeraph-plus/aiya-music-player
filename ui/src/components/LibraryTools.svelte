<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, RefreshCw, ScanSearch } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';
  import Icon from './Icon.svelte';
  import { errorMessage } from '../lib/errors';
  import { history } from '../stores/history.svelte';
  import { library } from '../stores/library.svelte';
  import { notify } from '../stores/notify.svelte';
  import { scan } from '../stores/scan.svelte';

  /**
   * 曲库段，2026-10-28 拍板收成**一张卡片**：
   *
   * 2026-10-30：宿主从「扫描入库」页换成设置页「曲库」组（那一页整体并入。
   * 本组件不依赖页面壳，只发动作、读 `library`/`scan` 两个 store，所以搬迁
   * 没有改一行逻辑，只换了宿主）。
   *
   * 「曲库目录」——目录条目列表 + 全部动作（添加目录 / 立即索引 / 清空
   * 曲库 / 清空播放历史）+ 内置报错 + 执行状态。textarea 已废：路径是
   * 整行增删的条目，不是可编辑文本；每行一个删除钮，删除走确认条
   * （连曲目清 or 仅停止监听）。
   *
   * 执行状态（分隔线以下）：进行中是"⟳ 曲库索引 · 阶段"一行 + 进度条 +
   * 数字明细，取消按钮就在同一行右端（全应用**唯一**的取消入口——曲库页
   * 的进度条只做环境指示，不带按钮）；结束后显示上次索引的记账。
   * 自动刷新是静默管线，不占这里的版面（用户拍板：那行文本没用）。
   * 扫描报错内置在这张卡（scan.lastError），不再弹全局 toast。
   *
   * 数据源约定不变：目录列表来自 `library.roots`（后端归一化后回填），
   * 本组件只发动作；删除曲目后走 `scan.afterScan()` 统一收尾（列表/统计/
   * 歌单一起刷，别只刷一半）。
   */
  let saving = $state(false);
  /** 目录卡片的内置报错（保存/删除/统计中的失败），下次动作开始时清除 */
  let rootsError = $state<string | null>(null);

  /**
   * 移除目录的确认态。
   *
   * 缺陷背景：直接覆盖写 `library.roots` 的话，目录下的曲目永久残留库里
   * （下轮扫描标 missing 而不清除）。现在删除先确认：可以连曲目一起清
   * （前缀删除，只删索引不动磁盘），也可以仅停止监听保留曲目。
   */
  interface PendingRemoval {
    paths: string[];
    count: number | null;
  }
  let pendingRemoval = $state<PendingRemoval | null>(null);

  /** 清空曲库的确认态（删除全部曲目记录：歌单壳保留，条目/历史级联清空） */
  let clearConfirm = $state(false);
  let clearing = $state(false);

  /** 两个确认条的取消键：出现时移入焦点（alertdialog 的键盘可达性） */
  let removalCancelEl = $state<HTMLButtonElement | null>(null);
  let clearCancelEl = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    if (pendingRemoval) removalCancelEl?.focus();
  });

  $effect(() => {
    if (clearConfirm) clearCancelEl?.focus();
  });

  const stats = $derived(library.stats);

  onMount(() => {
    void library.loadRoots();
  });

  /* ---------------- 目录条目操作 ---------------- */

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

  /** 保存根列表。失败内置显示（不回落草稿——没有草稿了，列表以 store 为准） */
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

  /* ---------------- 清空曲库 ---------------- */

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

  /* ---------------- 执行状态卡片 ---------------- */

  /**
   * 阶段文案（两段式进度）。
   *
   * 为什么需要它：解析与入库是**重叠的两段**，只报一个百分比时用户在
   * "解析快、入库慢"的那一段看到进度条不动，会以为卡死。这里把两段的
   * 主活动如实说出来，具体数字在下面一行。
   */
  function phaseText(phase: string): string {
    if (phase === 'done') return '已完成';
    if (phase === 'cancelled') return '已取消';
    return phase === 'walking' ? '遍历中' : phase === 'importing' ? '入库中' : '解析中';
  }
</script>

<div class="library-tools">
  <!-- 单卡片（七轮拍板）：动作按钮在头部右侧，目录条目 + 执行状态同卡 -->
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
        <!-- 清空播放历史（2026-10-28 从「高级与诊断」折叠区迁来）：
             它和清空曲库是同一族的"清掉一类记录"动作，放同一行。
             不加确认——历史只是播放记录，toast 即刻反馈，HomeView 同款语义 -->
        <Button size="sm" onclick={() => void history.clear()} disabled={clearing}>
          清空播放历史
        </Button>
      </div>
    </div>

    {#if rootsError}
      <div class="tool-error" role="alert">{rootsError}</div>
    {/if}

    {#if pendingRemoval}
      <div
        class="roots-confirm"
        role="alertdialog"
        aria-label="移除目录确认"
        tabindex="-1"
        onkeydown={(event) => {
          // 焦点在确认条内时 Esc 归它：stopPropagation 挡住 window 上的
          // 全局 Esc（否则关确认条的同时还会清掉列表选择集）
          if (event.key === 'Escape') {
            event.stopPropagation();
            dismissRemoval();
          }
        }}
      >
        <span class="roots-confirm-text">
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
          <button class="link" bind:this={removalCancelEl} onclick={dismissRemoval} disabled={saving}>取消</button>
        </div>
      </div>
    {/if}

    {#if clearConfirm}
      <div
        class="roots-confirm"
        role="alertdialog"
        aria-label="清空曲库确认"
        tabindex="-1"
        onkeydown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            clearConfirm = false;
          }
        }}
      >
        <span class="roots-confirm-text">
          将删除全部 <b>{stats?.tracks ?? 0}</b> 首曲目的索引记录，歌单内容、播放队列与播放历史一并清空
          （歌单本身保留；<b>磁盘文件不动</b>）。此操作不可撤销。
        </span>
        <div class="roots-confirm-actions">
          <Button size="sm" variant="danger" onclick={() => void doClear()} disabled={clearing}>
            确认清空
          </Button>
          <button class="link" bind:this={clearCancelEl} onclick={() => (clearConfirm = false)} disabled={clearing}>取消</button>
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
            <!-- 2026-10-30：这里原来有一个「整理输出」单选（写 beets 的
                 config.yaml）。它已经搬走——曲库页不提供 beets 的设置项，
                 两个库的配置各归各的（docs/48）。 -->
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

    <!-- 执行状态（并入本卡，分隔线隔开）。进行中：阶段与任务名一行、
         取消就在同一行右端；结束后只剩一行记账。没有标题行——
         "执行状态"三个字不携带信息，分隔线已经足够表达分区 -->
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
          <button class="link" onclick={() => scan.cancel()}>取消任务</button>
        </div>
        <div class="progress">
          <i style={`width:${progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8}%`}></i>
        </div>
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
