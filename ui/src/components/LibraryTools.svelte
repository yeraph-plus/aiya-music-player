<script lang="ts">
  import { onMount } from 'svelte';
  import { Download, ExternalLink, FolderOpen, RefreshCw, ScanSearch } from '@lucide/svelte';
  import BeetsMaintenance from './BeetsMaintenance.svelte';
  import ImportPanel from './ImportPanel.svelte';
  import Button from './ui/Button.svelte';
  import { beetsSession } from '../stores/beets-session.svelte';
  import { ipc } from '../lib/ipc';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { notify } from '../stores/notify.svelte';
  import { scan } from '../stores/scan.svelte';
  import { ui } from '../stores/ui.svelte';
  import { errorMessage } from '../lib/errors';

  /**
   * 扫描入库页的主体（docs/29 批 A 自设置页迁入）：多目录配置 + 扫描/beets
   * 任务进度 + 导入面板。
   *
   * 多目录（用户拍板：并集单库）：textarea 一行一个，失焦即存（后端再归一化：
   * trim / 去空行 / 去引号 / 大小写不敏感去重）。「选择目录」选完**追加**进
   * 列表（`library_choose_root` 已改为追加语义），不覆盖已有根。
   *
   * 文本框是**草稿态**：绑定本地 `rootsDraft`，失焦与 `library.roots` 对账
   * ——有效变更才发 roots_set；保存失败回落到 store 的值（后端归一化结果）。
   */
  let rootsDraft = $state('');
  let saving = $state(false);
  let lastSavedText = '';

  function draftFromRoots(roots: string[]): string {
    return roots.join('\n');
  }

  onMount(() => {
    void library.loadRoots();
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
      notify.showToast('扫描目录已保存');
    } catch (err) {
      notify.showError(errorMessage(err));
      rootsDraft = draftFromRoots(library.roots);
    } finally {
      saving = false;
    }
  }

  async function openBeetsWeb(): Promise<void> {
    try {
      await ipc.beets.webOpen();
    } catch (err) {
      notify.showError(errorMessage(err));
    }
  }

  /**
   * 阶段文案（docs/29 批 B 两段式进度）。
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
</script>

<div class="library-tools">
  <div class="library-roots">
    <div class="roots-head">
      <strong>扫描目录</strong>
      <span class="dim">{library.roots.length} 个 · 并集为同一曲库，一行一个</span>
    </div>
    <textarea
      class="roots-input"
      rows="4"
      spellcheck="false"
      aria-label="扫描目录列表，每行一个"
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
      <strong>扫描任务</strong>
      <p>对上面全部目录建立索引；再次扫描会更新变更并保留播放记录。</p>
    </div>
    <div class="library-scan-buttons">
      <Button size="sm" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
        {#snippet icon()}<FolderOpen size={14} />{/snippet}
        选择目录
      </Button>
      <Button variant="primary" size="sm" onclick={() => void scan.start()} disabled={library.scanning}>
        {#snippet icon()}<ScanSearch size={14} />{/snippet}
        {library.scanning ? '扫描中…' : '重新扫描'}
      </Button>
    </div>
  </div>

  {#if library.scan}
    {@const progress = library.scan}
    <div class="task-status">
      <div class="task-status-head">
        <span class="task-name"><RefreshCw size={13} class={library.scanning ? 'spinning' : undefined} /> 曲库扫描</span>
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
        {:else}
          {progress.scanned}/{progress.total ?? '?'} 已入库 · 解析 {progress.parsed}{#if progress.parsed > progress.scanned}<span class="dim">
            （排队 {progress.parsed - progress.scanned}）</span>{/if} · 新增 {progress.imported} · 跳过 {progress.skipped} · 失败 {progress.failed}
        {/if}
      </div>
      {#if library.scanning}<button class="link" onclick={() => scan.cancel()}>取消任务</button>{/if}
    </div>
  {:else}
    <div class="task-status task-idle">当前没有扫描任务。扫描与 beets 任务可以在这里直接查看进度和取消。</div>
  {/if}

  {#if beetsSession.phase !== 'idle'}
    <div class="task-status">
      <div class="task-status-head"><span>beets 任务</span><span>{beetsSession.phase}</span></div>
      {#if beetsSession.progress}
        <div class="task-detail">预览 {beetsSession.progress.current}/{beetsSession.progress.total}</div>
      {:else if beetsSession.applyProgress}
        <div class="task-detail">应用 {beetsSession.applyProgress.current}/{beetsSession.applyProgress.total}</div>
      {/if}
    </div>
  {/if}

  <ImportPanel onsynced={() => void list.reload({ restore: true })} />

  <BeetsMaintenance />

  <div class="beets-web-row">
    {#if ui.beets === null}
      <span class="dim">正在检测 beets 环境…</span>
    {:else if ui.beets.found}
      <Button size="sm" onclick={() => void openBeetsWeb()}>
        {#snippet icon()}<ExternalLink size={14} />{/snippet}
        在浏览器中打开 beets Web
      </Button>
      <span class="dim">高级用户出口：beets 自带 Web 界面（localhost:8337），应用内不做 GUI。</span>
    {:else if ui.beetsInstall.running}
      <span class="dim">
        正在安装 beets 运行时：{ui.beetsInstall.stage}
        {ui.beetsInstall.message ? `· ${ui.beetsInstall.message}` : ''}
      </span>
    {:else}
      <Button size="sm" onclick={() => void ui.installBeets()}>
        {#snippet icon()}<Download size={14} />{/snippet}
        安装 beets 运行时
      </Button>
      <span class="dim">
        {ui.beetsInstall.error ??
          '未检测到 beets。安装后可用导入整理、写标签与库对齐；库放在应用数据目录，不动你系统里的 beets。'}
      </span>
    {/if}
  </div>
</div>
