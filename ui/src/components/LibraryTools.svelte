<script lang="ts">
  import { FolderOpen, RefreshCw, ScanSearch } from '@lucide/svelte';
  import ImportPanel from './ImportPanel.svelte';
  import { beetsSession } from '../stores/beets-session.svelte';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { scan } from '../stores/scan.svelte';
</script>

<div class="library-tools">
  <div class="library-scan-actions">
    <div>
      <strong>扫描入库</strong>
      <p>选择音乐目录建立索引；再次扫描会更新变更并保留播放记录。</p>
    </div>
    <div class="library-scan-buttons">
      <button class="btn" onclick={() => void scan.pickRootAndScan()} disabled={library.scanning}>
        <FolderOpen size={14} /> 选择目录
      </button>
      <button class="btn primary" onclick={() => void scan.start()} disabled={library.scanning}>
        <ScanSearch size={14} /> {library.scanning ? '扫描中…' : '重新扫描'}
      </button>
    </div>
  </div>

  {#if library.scan}
    {@const progress = library.scan}
    <div class="task-status">
      <div class="task-status-head">
        <span class="task-name"><RefreshCw size={13} class={library.scanning ? 'spinning' : undefined} /> 曲库扫描</span>
        <span>{progress.phase === 'done' ? '已完成' : library.scanning ? '进行中' : progress.phase}</span>
      </div>
      {#if library.scanning}
        <div class="progress">
          <i style={`width:${progress.total ? Math.min(100, Math.round((progress.scanned / progress.total) * 100)) : 8}%`}></i>
        </div>
      {/if}
      <div class="task-detail">
        {progress.phase === 'walking'
          ? '正在遍历目录…'
          : `${progress.scanned}/${progress.total ?? '?'} · 新增 ${progress.imported} · 跳过 ${progress.skipped} · 失败 ${progress.failed}`}
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
</div>
