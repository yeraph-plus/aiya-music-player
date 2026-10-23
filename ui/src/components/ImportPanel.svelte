<script lang="ts">
  import type { AlbumPreview, BeetsImportEvent, CandidatePreview, ApplyDecision } from '@aiya/contracts';
  import Icon from './Icon.svelte';

  /**
   * beets 导入面板（B1 预览 + B2 应用）：
   * 选文件夹 → 专辑卡片 → 候选/逐字段 diff → 应用（写标签 + 入库 + 刷新曲库）。
   * 预览阶段零文件改动；应用阶段候选数据原样回传（所见即所写）。
   */

  interface Props {
    onclose: () => void;
    /** 读模型对齐完成（曲库数据可能变了），主视图应重拉列表 */
    onsynced?: () => void;
  }
  let { onclose, onsynced }: Props = $props();

  let albums = $state<AlbumPreview[]>([]);
  let running = $state(false);
  let applying = $state(false);
  let error = $state<string | null>(null);
  /** 专辑 id → 展开的候选 id（null = 显示当前标签） */
  let picked = $state<Record<string, string | null>>({});
  let progress = $state<{ current: number; total: number } | null>(null);
  let applyProgress = $state<{ current: number; total: number } | null>(null);
  /** 专辑 id → 应用结果 */
  let results = $state<Record<string, { written: number; failed: Array<{ path: string; message: string }> }>>({});
  let applySummary = $state<{ applied: number; skipped: number; failed: number } | null>(null);
  let synced = $state<{ matched: number; updated: number; imported: number; unchanged: number; orphaned: number } | null>(null);
  let syncing = $state(false);
  let doneCount = $state<number | null>(null);
  let errors = $state<Array<{ path: string; message: string }>>([]);
  let unlisten: (() => void) | null = null;

  function onEvent(evt: BeetsImportEvent): void {
    if (evt.type === 'album') {
      albums = [...albums, evt.album];
      // 默认选择：只有 strong 才默认选第一个候选（docs/08 §4.1 低置信度不默认勾选）
      const strong = evt.album.candidates[0];
      picked[evt.album.id] = evt.album.recommendation === 'strong' && strong ? strong.id : null;
    } else if (evt.type === 'progress') {
      if (evt.stage === 'writing') applyProgress = { current: evt.current, total: evt.total };
      else progress = { current: evt.current, total: evt.total };
    } else if (evt.type === 'albumError') {
      errors = [...errors, { path: evt.path, message: evt.beets_message }];
    } else if (evt.type === 'applied') {
      results = { ...results, [evt.albumId]: { written: evt.written, failed: evt.failed } };
    } else if (evt.type === 'applyDone') {
      applying = false;
      applySummary = { applied: evt.applied, skipped: evt.skipped, failed: evt.failed };
    } else if (evt.type === 'librarySynced') {
      syncing = false;
      synced = { matched: evt.matched, updated: evt.updated, imported: evt.imported, unchanged: evt.unchanged, orphaned: evt.orphaned };
      // 曲库数据变了（新增/更新行），让主视图重拉列表
      onsynced?.();
    } else if (evt.type === 'librarySyncFailed') {
      syncing = false;
      errors = [...errors, { path: 'sync', message: evt.message }];
    } else if (evt.type === 'libraryRefreshSkipped') {
      syncing = false;
      errors = [...errors, { path: 'sync', message: '有扫描正在进行，本次对齐让位；稍后再试' }];
    } else if (evt.type === 'done') {
      doneCount = evt.albums;
      running = false;
    } else if (evt.type === 'sessionEnd') {
      running = false;
    }
  }

  async function chooseAndPreview(): Promise<void> {
    error = null;
    const picked_dir = await window.api.library.chooseRoot();
    if (!picked_dir.ok || !picked_dir.data) return;
    // 重置状态，订阅事件，发起预览
    albums = [];
    picked = {};
    progress = null;
    applyProgress = null;
    results = {};
    applySummary = null;
    synced = null;
    syncing = false;
    doneCount = null;
    errors = [];
    const result = await window.api.app.beetsPreview([picked_dir.data.path]);
    if (!result.ok) {
      error = result.error.message;
      return;
    }
    running = true;
  }

  /** 单张专辑的决策：有选中候选就应用，否则跳过 */
  function decisionFor(album: AlbumPreview): ApplyDecision {
    const cand = candidateMeta(album);
    if (!cand) return { album: album.id, action: 'skip', paths: album.paths };
    return {
      album: album.id,
      action: 'apply',
      paths: album.paths,
      candidate: { album: cand.album, mapping: cand.mapping },
    };
  }

  const applyableCount = $derived(albums.filter((a) => candidateMeta(a)).length);

  /** 应用全部有候选的专辑（其余按 skip 记账） */
  async function applyAll(): Promise<void> {
    await applyDecisions(albums.map(decisionFor));
  }

  /** 只应用一张专辑 */
  async function applyOne(album: AlbumPreview): Promise<void> {
    await applyDecisions(
      albums.map((a) => (a.id === album.id ? decisionFor(a) : { album: a.id, action: 'skip', paths: a.paths })),
    );
  }

  async function applyDecisions(decisions: ApplyDecision[]): Promise<void> {
    error = null;
    results = {};
    applySummary = null;
    synced = null;
    const result = await window.api.app.beetsApply(decisions);
    if (!result.ok) {
      error = result.error.message;
      return;
    }
    applying = true;
    syncing = true;
  }

  /** 手动全量对齐：beets 库 → 自有读模型（无需先预览） */
  async function syncNow(): Promise<void> {
    error = null;
    synced = null;
    const result = await window.api.app.beetsSync();
    if (!result.ok) {
      error = result.error.message;
      return;
    }
    syncing = true;
  }

  $effect(() => {
    const off = window.api.app.onBeetsImport(onEvent);
    unlisten = off;
    return () => {
      off();
      unlisten = null;
    };
  });

  function currentMeta(album: AlbumPreview) {
    return album.current;
  }
  function candidateMeta(album: AlbumPreview): CandidatePreview | null {
    const id = picked[album.id];
    return album.candidates.find((c) => c.id === id) ?? null;
  }
  function fieldDiff(current: unknown, candidate: unknown): boolean {
    return String(current ?? '') !== String(candidate ?? '');
  }

  const recLabel: Record<string, string> = {
    strong: '高置信',
    medium: '中置信',
    low: '低置信',
    none: '无匹配',
  };
</script>

<div class="side-panel">
  <header class="panel-head">
    <h2>导入整理（beets）</h2>
    <button class="icon-btn" onclick={onclose} aria-label="关闭">✕</button>
  </header>

  <div class="import-body">
    <div class="import-actions">
      <button class="btn primary" onclick={() => void chooseAndPreview()} disabled={running || applying}>
        <Icon name="folder" size={13} /> 选择文件夹预览
      </button>
      {#if running}
        <span class="dim">
          匹配中 {progress ? `${progress.current}/${progress.total}` : '…'}（MusicBrainz，只读不改文件）
        </span>
      {/if}
      {#if doneCount !== null && !running}
        <span class="dim">预览完成：{doneCount} 个文件夹分组</span>
      {/if}
      {#if applying}
        <span class="dim">
          写入标签 {applyProgress ? `${applyProgress.current}/${applyProgress.total}` : '…'}（不动文件位置）
        </span>
      {/if}
      {#if doneCount !== null && !running && !applying}
        <button class="btn primary" onclick={() => void applyAll()} disabled={applyableCount === 0}>
          应用所选（{applyableCount} 张）
        </button>
      {/if}
      <button class="btn" onclick={() => void syncNow()} disabled={syncing} title="beets 库 → 本地曲库全量对齐">
        从 beets 同步
      </button>
    </div>

    {#if applySummary}
      <div class="apply-summary" class:has-failed={applySummary.failed > 0}>
        已应用 {applySummary.applied} 张 · 跳过 {applySummary.skipped} 张
        {#if applySummary.failed > 0} · 失败 {applySummary.failed} 首{/if}
        {#if synced}
          <div class="dim">
            曲库已对齐：命中 {synced.matched} · 更新 {synced.updated} · 新增 {synced.imported} · 未变 {synced.unchanged}
            {#if synced.orphaned > 0} · beets 已删 {synced.orphaned}{/if}
          </div>
        {:else if syncing}
          <div class="dim">正在对齐曲库（beets → 本地）…</div>
        {/if}
      </div>
    {:else if synced}
      <div class="apply-summary">
        曲库已对齐：命中 {synced.matched} · 更新 {synced.updated} · 新增 {synced.imported} · 未变 {synced.unchanged}
        {#if synced.orphaned > 0} · beets 已删 {synced.orphaned}{/if}
      </div>
    {/if}

    {#if error}
      <div class="import-error">
        {error}
        <div class="dim">
          需要 beets 2.14+：<code>pipx install beets</code>（装好重启应用）
        </div>
      </div>
    {/if}

    {#if errors.length > 0}
      <div class="import-error">
        {#each errors as e (e.path + e.message)}
          <div>{e.path}：{e.message}</div>
        {/each}
      </div>
    {/if}

    {#if albums.length === 0 && !running && doneCount === null}
      <div class="dim empty-hint">
        选择一个待整理的文件夹，beets 会只读匹配候选元数据并展示逐字段差异；
        预览阶段不写任何文件。
      </div>
    {/if}

    {#each albums as album (album.id)}
      <div class="album-card">
        <div class="album-head">
          <b>{album.current.album || '(未知专辑)'}</b>
          <span class="dim">{album.current.artist || '(未知艺人)'} · {album.paths.length} 首</span>
          <span class="rec" class:strong={album.recommendation === 'strong'}>
            {recLabel[album.recommendation] ?? album.recommendation}
          </span>
        </div>

        <!-- 应用结果徽标（B2） -->
        {#if results[album.id]}
          <div class="apply-result" class:has-failed={results[album.id].failed.length > 0}>
            ✓ 已写入 {results[album.id].written} 首
            {#each results[album.id].failed as f (f.path)}
              <div class="dim">✗ {f.path.split(/[\\/]/).pop()}：{f.message}</div>
            {/each}
          </div>
        {/if}

        <!-- 候选选择 -->
        {#if album.candidates.length === 0}
          <div class="dim">无候选（未联网或匹配不上）</div>
        {:else if !results[album.id]}
          <select
            class="cand-select"
            value={picked[album.id] ?? ''}
            disabled={applying}
            onchange={(e) => (picked[album.id] = (e.currentTarget as HTMLSelectElement).value || null)}
          >
            <option value="">保持当前标签（不修改）</option>
            {#each album.candidates as c (c.id)}
              <option value={c.id}>
                {c.album.album ?? '?'} — {c.album.artist ?? '?'} {c.album.year ?? ''}（{(c.confidence * 100).toFixed(0)}%）
              </option>
            {/each}
          </select>

          {#if candidateMeta(album)}
            <table class="diff-table">
              <thead><tr><th>字段</th><th>当前</th><th>候选</th></tr></thead>
              <tbody>
                {#each ['album', 'artist', 'albumartist', 'year', 'genre'] as f (f)}
                  <tr>
                    <td>{f}</td>
                    <td>{String((album.current as unknown as Record<string, unknown>)[f] ?? '')}</td>
                    <td class:changed={fieldDiff((album.current as unknown as Record<string, unknown>)[f], (candidateMeta(album)!.album as unknown as Record<string, unknown>)[f])}>
                      {String((candidateMeta(album)!.album as unknown as Record<string, unknown>)[f] ?? '')}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <details class="track-diff">
              <summary class="dim">逐曲目（{Object.keys(candidateMeta(album)!.mapping).length} 首对齐）</summary>
              {#each currentMeta(album).tracks as t (t.path)}
                <div class="track-row">
                  <span class="dim">{t.title || t.path?.split(/[\\/]/).pop()}</span>
                  <span>
                    {candidateMeta(album)!.mapping[t.path ?? '']?.title ?? '(对不上)'}
                  </span>
                </div>
              {/each}
            </details>
            <button
              class="btn apply-btn"
              onclick={() => void applyOne(album)}
              disabled={applying || running}
            >
              应用此候选（写标签）
            </button>
          {/if}
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .import-body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
    overflow-y: auto;
  }
  .import-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .import-error {
    border: 1px solid var(--danger, #c33);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
  }
  .empty-hint {
    font-size: 12px;
    line-height: 1.6;
  }
  .album-card {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
  }
  .album-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }
  .rec {
    margin-left: auto;
    font-size: 10.5px;
    color: var(--text-dim);
  }
  .rec.strong {
    color: var(--accent);
  }
  .cand-select {
    width: 100%;
    font-size: 12px;
  }
  .diff-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11.5px;
  }
  .diff-table td,
  .diff-table th {
    padding: 2px 6px;
    text-align: left;
    border-bottom: 1px solid var(--border);
  }
  .diff-table .changed {
    color: var(--accent);
    font-weight: 600;
  }
  .track-diff {
    font-size: 11.5px;
  }
  .track-row {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 1px 0;
  }
  .apply-btn {
    align-self: flex-start;
    font-size: 11.5px;
  }
  .apply-summary {
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
    line-height: 1.6;
  }
  .apply-summary.has-failed {
    border-color: var(--danger, #c33);
  }
  .apply-result {
    color: var(--accent);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .apply-result.has-failed {
    color: var(--text);
  }
</style>
