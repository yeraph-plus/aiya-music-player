<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, RefreshCw, ScanSearch } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import { errorMessage } from '../lib/errors';
  import { formatBytes, formatDateTime, formatDurationLong } from '../lib/format';
  import { library } from '../stores/library.svelte';
  import { notify } from '../stores/notify.svelte';
  import { scan } from '../stores/scan.svelte';

  /**
   * 曲库段（扫描入库页第一组）。
   *
   * 这一组只管**播放器自己的曲库**：目录 → 建立索引 → 概况 → 自动刷新。
   *
   * 为什么和 beets 分开（本轮信息架构整理的核心）：以前这一页把两组东西
   * 排在一起，而它们的**数据源不同**——这里是 `tracks` 表，那边是 beets 的
   * `library.db`。混排的直接后果是两个都叫"库"的东西看不出区别：
   * 「扫描目录」进的是曲库，「导入目录」进的是整理库；「重新扫描」重读
   * 文件元数据，「刷新标签」却会让 beets 按 `path_formats` 重写文件。
   *
   * 多目录（用户拍板：并集单库）：textarea 一行一个，失焦即存（后端再归一化：
   * trim / 去空行 / 去引号 / 大小写不敏感去重）。「选择目录」选完**追加**进
   * 列表（`library_choose_root` 已是追加语义），不覆盖已有根。
   *
   * 文本框是**草稿态**：绑定本地 `rootsDraft`，失焦与 `library.roots` 对账
   * ——有效变更才发 roots_set；保存失败回落到 store 的值（后端归一化结果）。
   */
  let rootsDraft = $state('');
  let saving = $state(false);
  let lastSavedText = '';

  const stats = $derived(library.stats);
  const refresh = $derived(scan.refresh);
  const lastRefresh = $derived(refresh?.last ?? null);

  function draftFromRoots(roots: string[]): string {
    return roots.join('\n');
  }

  onMount(() => {
    void library.loadRoots();
    void scan.loadRefreshState();
  });

  // store 值变化（loadRoots 完成 / chooseRoot 追加 / 保存回填）→ 草稿跟随，
  // 但用户已改过草稿（草稿 ≠ 上次已存文本）时不打断输入
  $effect(() => {
    const stored = draftFromRoots(library.roots);
    if (rootsDraft !== stored && rootsDraft === lastSavedText) rootsDraft = stored;
    lastSavedText = stored;
  });

  /** 失焦保存：草稿与当前已存文本不同才发（含顺序变化——顺序有语义，保留） */
  async function commitRoots(): Promise<void> {
    const stored = draftFromRoots(library.roots);
    if (rootsDraft === stored || saving) return;
    saving = true;
    try {
      await library.saveRoots(rootsDraft.split('\n'));
      // 草稿回填为**后端归一化结果**（去重/去空行），textarea 与实际生效值一致
      rootsDraft = draftFromRoots(library.roots);
      notify.showToast('曲库目录已保存');
    } catch (err) {
      notify.showError(errorMessage(err));
      rootsDraft = draftFromRoots(library.roots);
    } finally {
      saving = false;
    }
  }

  /**
   * 阶段文案（两段式进度）。
   *
   * 为什么需要它：解析与入库是**重叠的两段**，只报一个百分比时用户在
   * "解析快、入库慢"的那一段看到进度条不动，会以为卡死。这里把两段的
   * 主活动如实说出来，具体数字在下面一行。
   */
  function phaseText(phase: string, scanning: boolean): string {
    if (phase === 'done') return '已完成';
    if (phase === 'cancelled') return '已取消';
    if (!scanning) return phase;
    return phase === 'walking' ? '遍历中' : phase === 'importing' ? '入库中' : '解析中';
  }

  /** 自动刷新的一句话状态。读的是"上一次跑成了什么样"，不是配置 */
  function refreshText(): string {
    if (!refresh) return '—';
    const cadence =
      refresh.intervalSecs > 0 ? `每 ${refresh.intervalSecs} 秒` : '仅切回窗口时';
    if (!lastRefresh) return `${cadence} · 还没跑过`;
    const at = formatDateTime(lastRefresh.atMs);
    if (lastRefresh.error) return `${cadence} · 上次失败`;
    const parts: string[] = [];
    if (lastRefresh.imported > 0) parts.push(`新增 ${lastRefresh.imported}`);
    if (lastRefresh.updated > 0) parts.push(`更新 ${lastRefresh.updated}`);
    if (lastRefresh.missing > 0) parts.push(`离线 ${lastRefresh.missing}`);
    const tail = parts.length === 0 ? '无变化' : parts.join(' · ');
    return `${cadence} · 上次 ${at} ${tail}`;
  }
</script>

<div class="library-tools">
  <div class="library-roots">
    <div class="roots-head">
      <strong>曲库目录</strong>
      <span class="dim">{library.roots.length} 个目录 · 并集为同一个曲库，一行一个</span>
    </div>
    <textarea
      class="roots-input"
      rows="4"
      spellcheck="false"
      aria-label="曲库目录列表，每行一个"
      placeholder={'D:\\Music\nD:\\Music-Lossless'}
      bind:value={rootsDraft}
      onblur={() => void commitRoots()}
    ></textarea>
    <div class="roots-hint dim">
      空行与重复目录自动忽略；路径不存在时扫描会跳过并提示。改动失焦即保存。
    </div>
  </div>

  <div class="library-scan-actions">
    <div>
      <strong>建立索引</strong>
      <p>读文件里的标签与时长，把曲目收进本机数据库。只读文件，不改任何标签。</p>
    </div>
    <div class="library-scan-buttons">
      <Button size="sm" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
        {#snippet icon()}<FolderOpen size={14} />{/snippet}
        添加目录
      </Button>
      <Button variant="primary" size="sm" onclick={() => void scan.start()} disabled={library.scanning}>
        {#snippet icon()}<ScanSearch size={14} />{/snippet}
        {library.scanning ? '索引中…' : '立即索引'}
      </Button>
    </div>
  </div>

  {#if library.scan}
    {@const progress = library.scan}
    <div class="task-status">
      <div class="task-status-head">
        <span class="task-name"><RefreshCw size={13} class={library.scanning ? 'spinning' : undefined} /> 曲库索引</span>
        <span>{phaseText(progress.phase, library.scanning)}</span>
      </div>
      {#if library.scanning}
        <div class="progress">
          <i style={`width:${progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8}%`}></i>
        </div>
      {/if}
      <div class="task-detail">
        {#if progress.phase === 'walking'}
          正在遍历目录…
        {:else if library.scanning}
          {progress.scanned}/{progress.total ?? '?'} 已入库 · 解析 {progress.parsed}{#if progress.parsed > progress.scanned}<span class="dim">
            （排队 {progress.parsed - progress.scanned}）</span>{/if} · 新增 {progress.imported} · 更新 {progress.updated} · 跳过 {progress.skipped} · 失败 {progress.failed}
        {:else}
          本次：新增 {progress.imported} · 更新 {progress.updated} · 离线 {progress.missing} · 跳过 {progress.skipped}{#if progress.failed} · 失败 {progress.failed}{/if} · {Math.round(progress.elapsedMs / 100) / 10} 秒
        {/if}
      </div>
      {#if library.scanning}<button class="link" onclick={() => scan.cancel()}>取消任务</button>{/if}
    </div>
  {/if}

  <div class="library-facts">
    {#if stats}
      <div class="facts-row">
        <span><b>{stats.tracks}</b> 首</span>
        <span><b>{stats.albums}</b> 专辑</span>
        <span><b>{stats.artists}</b> 位艺人</span>
        <span>{formatDurationLong(stats.totalDurationMs)}</span>
        {#if stats.missingLocations > 0}
          <span class="danger">{stats.missingLocations} 首离线</span>
        {/if}
      </div>
    {:else}
      <div class="dim">正在读取曲库统计…</div>
    {/if}
    <div class="refresh-row">
      <span class="refresh-label"><RefreshCw size={11} /> 自动刷新</span>
      <span class="dim">{refreshText()}</span>
    </div>
    {#if stats}
      <div class="facts-foot dim">
        索引占 {formatBytes(stats.dbBytes)} · schema v{stats.schemaVersion}
        {#if refresh?.focused === false}· 窗口在后台，定时暂停{/if}
      </div>
    {/if}
  </div>
</div>
