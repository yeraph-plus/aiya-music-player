<script lang="ts">
  import type { AlbumPreview, CandidatePreview } from '../lib/beets';
  import type { ApplyDecision } from '../lib/ipc';
  import { beetsSession as beets } from '../stores/beets-session.svelte';
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';

  /**
   * beets 导入面板（B1 预览 + B2 应用 + B3 移动/复制/取消）：
   * 选文件夹 → 专辑卡片 → 候选/逐字段 diff → 应用（写标签，可选移动/复制
   * 到 beets 库）→ 取消随时可用（收尾到最近检查点）。
   * 预览阶段零文件改动；应用阶段候选数据原样回传（所见即所写）。
   *
   * 会话状态（phase/进度/错误/结果）全部来自 stores/beets-session.svelte.ts：
   * 事件监听在 store 模块级注册，不随本组件生命周期；这里只发起命令
   * （beets_preview/beets_apply/beets_cancel/beets_sync）并保留局部 UI 态
   * （文件动作选择、逐专辑候选选择）。
   */

  interface Props {
    onclose?: () => void;
    /** 读模型对齐完成（曲库数据可能变了），主视图应重拉列表 */
    onsynced?: () => void;
  }
  let { onclose, onsynced }: Props = $props();

  /** 文件动作（docs/08 §4.1：默认只写标签，移动/复制显式选择） */
  type MoveMode = 'apply' | 'apply_copy' | 'apply_move';
  /**
   * 应用时的文件动作。三种都要说得清"文件会不会动"——这是这一屏最危险的选择：
   * 用户以为只是改标签，实际把两万首歌搬进了另一个盘。
   */
  const MOVE_LABELS: Record<MoveMode, string> = {
    apply: '只写标签，文件不动',
    apply_copy: '复制一份到整理库',
    apply_move: '移动到整理库',
  };

  /** 文件动作：默认只写标签（B3 移动/复制显式选择） */
  let moveMode = $state<MoveMode>('apply');

  const running = $derived(beets.phase === 'previewing');
  const applying = $derived(beets.phase === 'applying');
  const syncing = $derived(beets.phase === 'syncing');

  // store 的 syncVersion 递增 = 一次新的曲库对齐完成 → 通知主视图重拉。
  // 以挂载时的版本为基线，避免打开面板时被旧会话的对齐结果误触发。
  let seenSyncVersion = beets.syncVersion;
  $effect(() => {
    if (beets.syncVersion === seenSyncVersion) return;
    seenSyncVersion = beets.syncVersion;
    onsynced?.();
  });

  // 取数全部在 store 里：本组件只发动作、只读快照（docs/18 §4.1）
  const chooseAndPreview = (): Promise<void> => beets.chooseAndPreview();

  /** 单张专辑的决策：有选中候选就应用，否则跳过（动作跟随 moveMode） */
  function decisionFor(album: AlbumPreview): ApplyDecision {
    const cand = candidateMeta(album);
    if (!cand) return { album: album.id, action: 'skip', paths: album.paths };
    return {
      album: album.id,
      action: moveMode,
      paths: album.paths,
      candidate: { album: cand.album, mapping: cand.mapping },
    };
  }

  const applyableCount = $derived(beets.albums.filter((a) => candidateMeta(a)).length);

  /** 应用全部有候选的专辑（其余按 skip 记账） */
  async function applyAll(): Promise<void> {
    await applyDecisions(beets.albums.map(decisionFor));
  }

  /** 只应用一张专辑 */
  async function applyOne(album: AlbumPreview): Promise<void> {
    await applyDecisions(
      beets.albums.map((a) => (a.id === album.id ? decisionFor(a) : { album: a.id, action: 'skip', paths: a.paths })),
    );
  }

  async function applyDecisions(decisions: ApplyDecision[]): Promise<void> {
    await beets.apply(decisions);
  }

  /** 取消当前预览/应用：sidecar 收尾到最近检查点，已完成部分保留 */
  const cancelSession = (): Promise<void> => beets.cancel();

  /** 手动全量对齐：beets 库 → 自有读模型（无需先预览） */
  const syncNow = (): Promise<void> => beets.syncNow();

  function currentMeta(album: AlbumPreview) {
    return album.current;
  }
  function candidateMeta(album: AlbumPreview): CandidatePreview | null {
    const id = beets.picked[album.id];
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
    <h2>导入新文件夹</h2>
    {#if onclose}
      <IconButton label="关闭" size="sm" onclick={onclose}>
        <Icon name="close" size={13} />
      </IconButton>
    {/if}
  </header>

  <div class="import-body">
    <div class="import-actions">
      <Button variant="primary" size="sm" onclick={() => void chooseAndPreview()} disabled={running || applying}>
        {#snippet icon()}<Icon name="folder" size={13} />{/snippet}
        选择文件夹
      </Button>
      {#if running}
        <span class="dim">
          匹配中 {beets.progress ? `${beets.progress.current}/${beets.progress.total}` : '…'}（MusicBrainz，只读不改文件）
        </span>
      {/if}
      {#if beets.doneCount !== null && !running}
        <span class="dim">预览完成：{beets.doneCount} 个文件夹分组</span>
      {/if}
      {#if applying}
        <span class="dim">
          {MOVE_LABELS[moveMode]} {beets.applyProgress ? `${beets.applyProgress.current}/${beets.applyProgress.total}` : '…'}
        </span>
      {/if}
      {#if running || applying}
        <Button size="sm" onclick={() => void cancelSession()} ariaLabel="取消当前 beets 导入">取消</Button>
      {/if}
      {#if beets.doneCount !== null && !running && !applying}
        <select
          class="cand-select mode-select"
          value={moveMode}
          onchange={(e) => (moveMode = (e.currentTarget as HTMLSelectElement).value as MoveMode)}
          title="移动/复制按 beets 的 path_formats 计算目标路径；目标已存在时该曲目不覆盖，报冲突"
        >
          {#each Object.entries(MOVE_LABELS) as [mode, label] (mode)}
            <option value={mode}>{label}</option>
          {/each}
        </select>
        <Button variant="primary" size="sm" onclick={() => void applyAll()} disabled={applyableCount === 0}>
          应用所选（{applyableCount} 张）
        </Button>
      {/if}
      <Button size="sm" onclick={() => void syncNow()} disabled={syncing} title="把整理库里的元数据对齐回曲库（与自动刷新是同一条管线）">从整理库对齐</Button>
    </div>

    {#if beets.applySummary}
      <div class="apply-summary" class:has-failed={beets.applySummary.failed > 0}>
        已应用 {beets.applySummary.applied} 张 · 跳过 {beets.applySummary.skipped} 张
        {#if beets.applySummary.moved > 0} · 已{MOVE_LABELS[moveMode].replace('到 beets 库', '')} {beets.applySummary.moved} 首{/if}
        {#if beets.applySummary.failed > 0} · 失败 {beets.applySummary.failed} 首{/if}
        {#if beets.synced}
          <div class="dim">
            曲库已对齐：命中 {beets.synced.matched} · 更新 {beets.synced.updated} · 新增 {beets.synced.imported} · 未变 {beets.synced.unchanged}
            {#if beets.synced.orphaned > 0} · beets 已删 {beets.synced.orphaned}{/if}
          </div>
        {:else if syncing}
          <div class="dim">正在对齐曲库（beets → 本地）…</div>
        {/if}
      </div>
    {:else if beets.synced}
      <div class="apply-summary">
        曲库已对齐：命中 {beets.synced.matched} · 更新 {beets.synced.updated} · 新增 {beets.synced.imported} · 未变 {beets.synced.unchanged}
        {#if beets.synced.orphaned > 0} · beets 已删 {beets.synced.orphaned}{/if}
      </div>
    {/if}

    {#if beets.lastError}
      <div class="import-error">
        {beets.lastError}
        <div class="dim">若提示组件缺失或版本不符，到上方「整理库」区块重装。</div>
      </div>
    {/if}

    {#if beets.errors.length > 0}
      <div class="import-error">
        {#each beets.errors as e (e.path + e.message)}
          <div>{e.path}：{e.message}</div>
        {/each}
      </div>
    {/if}

    {#if beets.albums.length === 0 && !running && beets.doneCount === null}
      <div class="dim empty-hint">
        选一个待整理的文件夹：会联网匹配候选元数据并展示逐字段差异，
        预览阶段不写任何文件。
      </div>
    {/if}

    {#each beets.albums as album (album.id)}
      <div class="album-card">
        <div class="album-head">
          <b>{album.current.album || '(未知专辑)'}</b>
          <span class="dim">{album.current.artist || '(未知艺人)'} · {album.paths.length} 首</span>
          <span class="rec" class:strong={album.recommendation === 'strong'}>
            {recLabel[album.recommendation] ?? album.recommendation}
          </span>
        </div>

        <!-- 应用结果徽标（B2 写标签 + B3 移动/复制/冲突） -->
        {#if beets.results[album.id]}
          <div class="apply-result" class:has-failed={beets.results[album.id].failed.length > 0}>
            ✓ 已写入 {beets.results[album.id].written} 首
            {#if beets.results[album.id].moved}
              · 文件已{moveMode === 'apply_move' ? '移动' : '复制'}到 beets 库
            {/if}
            {#each beets.results[album.id].failed as f (f.path)}
              <div class="fail-line" class:conflict={f.code === 'conflict'}>
                {f.code === 'conflict' ? '⚠ 目标冲突' : '✗'}
                {f.path.split(/[\\/]/).pop()}：{f.message}
              </div>
            {/each}
          </div>
        {/if}

        <!-- 候选选择 -->
        {#if album.candidates.length === 0}
          <div class="dim">无候选（未联网或匹配不上）</div>
        {:else if !beets.results[album.id]}
          <select
            class="cand-select"
            value={beets.picked[album.id] ?? ''}
            disabled={applying}
            onchange={(e) => (beets.picked[album.id] = (e.currentTarget as HTMLSelectElement).value || null)}
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
            <div class="apply-row">
              <Button size="sm" onclick={() => void applyOne(album)} disabled={applying || running}>
                应用此候选（{MOVE_LABELS[moveMode]}）
              </Button>
            </div>
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
    border: 1px solid var(--danger);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
  }
  .empty-hint {
    font-size: 12px;
    line-height: 1.6;
  }
  .album-card {
    border: 1px solid var(--line);
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
    color: var(--text-2);
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
    border-bottom: 1px solid var(--line);
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
  .apply-row {
    align-self: flex-start;
  }
  .apply-summary {
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
    line-height: 1.6;
  }
  .apply-summary.has-failed {
    border-color: var(--danger);
  }
  .apply-result {
    color: var(--accent);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .apply-result.has-failed {
    color: var(--text-1);
  }
  .fail-line.conflict {
    color: var(--warn);
  }
  .mode-select {
    width: auto;
  }
</style>
