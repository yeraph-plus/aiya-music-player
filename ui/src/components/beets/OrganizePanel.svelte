<script lang="ts">
  import { onMount } from 'svelte';
  import Chip from '../ui/Chip.svelte';
  import { Check, Folder, FolderOpen, Inbox, Play, RotateCcw, TriangleAlert, X } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import Select from '../ui/Select.svelte';
  import type { AlbumPreview, CandidatePreview, EvidenceTier } from '../../lib/beets';
  import type { ApplyDecision } from '../../lib/ipc';
  import { errorMessage } from '../../lib/errors';
  import { shortenPath } from '../../lib/format';
  import { ipc } from '../../lib/ipc';
  import { beetsConfig } from '../../stores/beets-config.svelte';
  import { beetsSession as sess } from '../../stores/beets-session.svelte';
  import { library } from '../../stores/library.svelte';
  import { t, type Msg } from '../../lib/i18n.svelte';

  /**
   * 整理子页：**待整理目录 → 预览 → 落点 → 移动或拷贝**。
   *
   * ## 三个目录各管什么
   *
   * - **待整理目录**（inbox）：还没进曲库的新文件丢这里。必须与曲库目录不重叠
   *   ——copy 模式下原件留在里面，重叠就会被扫描器再扫一遍变成重复曲目。
   * - **落点**：整理后的文件落到哪。必须**在曲库目录之内**（可以是子目录），
   *   否则整理成功了但扫描器看不见。
   * - **曲库目录**：应用自己的扫描根，这一页只读不写。
   *
   * ## 动作是每次选的，不写进配置
   *
   * 拷贝 / 移动是**这一批**的选择，不是长期偏好。默认「拷贝」：移动不可逆
   * （源文件没了），第一次用的人应该能反悔。这是安全默认，不是性能选择。
   *
   * 逐张专辑只选候选（或跳过），不逐张选动作——两套并存的"到底哪个生效"
   * 会让这张卡变成谜题。需要逐张不同动作时先分两次整理，比猜规则便宜。
   *
   * ## 预览阶段零写入
   *
   * 匹配是联网查 MusicBrainz，只读；应用阶段候选数据原样回传，不重跑网络
   * （所见即所写）。
   */

  type FileAction = 'apply_copy' | 'apply_move';

  const ACTION_KEY: Record<FileAction, Msg> = {
    apply_copy: 'beets.organize.actionCopy',
    apply_move: 'beets.organize.actionMove',
  };

  /** 默认拷贝：移动不可逆 */
  let action = $state<FileAction>('apply_copy');

  let inboxError = $state<string | null>(null);
  let targetError = $state<string | null>(null);

  const inbox = $derived(beetsConfig.inbox);
  const target = $derived(beetsConfig.effectiveTarget);
  const running = $derived(sess.phase === 'previewing');
  const applying = $derived(sess.phase === 'applying');
  const busy = $derived(running || applying);

  onMount(() => {
    void beetsConfig.load();
    void library.loadRoots();
  });

  /* 待整理目录 */

  async function chooseInbox(): Promise<void> {
    inboxError = null;
    let path: string | null = null;
    try {
      // pickFolder 而不是 library.chooseRoot：后者会把选中的目录写进曲库目录
      path = await ipc.library.pickFolder();
    } catch (e) {
      inboxError = errorMessage(e);
      return;
    }
    if (!path) return; // 用户取消
    try {
      await beetsConfig.setInbox(path);
    } catch (e) {
      inboxError = errorMessage(e);
    }
  }

  async function openInbox(): Promise<void> {
    inboxError = null;
    try {
      await beetsConfig.openInbox();
    } catch (e) {
      inboxError = errorMessage(e);
    }
  }

  async function clearInbox(): Promise<void> {
    inboxError = null;
    try {
      await beetsConfig.setInbox(null);
    } catch (e) {
      inboxError = errorMessage(e);
    }
  }

  /* 落点 */

  async function setTarget(path: string): Promise<void> {
    targetError = null;
    try {
      await beetsConfig.setTarget(path);
    } catch (e) {
      targetError = errorMessage(e);
    }
  }

  async function chooseTarget(): Promise<void> {
    targetError = null;
    let path: string | null = null;
    try {
      path = await ipc.library.pickFolder();
    } catch (e) {
      targetError = errorMessage(e);
      return;
    }
    if (!path) return;
    await setTarget(path);
  }

  /* 预览与应用 */

  function startPreview(): void {
    if (!inbox) return;
    void sess.previewInbox(inbox);
  }

  /** 有选中候选才算"会写"；其余按 skip 记账，界面上一眼能看出会动几张 */
  function candidateOf(album: AlbumPreview): CandidatePreview | null {
    const id = sess.picked[album.id];
    return album.candidates.find((c) => c.id === id) ?? null;
  }

  const willApply = $derived(sess.albums.filter((a) => candidateOf(a) !== null).length);

  function decisionFor(album: AlbumPreview): ApplyDecision {
    const cand = candidateOf(album);
    if (!cand) return { album: album.id, action: 'skip', paths: album.paths };
    return {
      album: album.id,
      action,
      paths: album.paths,
      candidate: { album: cand.album, mapping: cand.mapping },
    };
  }

  async function applyAll(): Promise<void> {
    await sess.apply(sess.albums.map(decisionFor));
  }

  async function applyOne(album: AlbumPreview): Promise<void> {
    await sess.apply(
      sess.albums.map((a) =>
        a.id === album.id ? decisionFor(a) : { album: a.id, action: 'skip', paths: a.paths },
      ),
    );
  }

  /* 展示用 */

  const REC_KEY: Record<string, Msg> = {
    strong: 'beets.organize.recStrong',
    medium: 'beets.organize.recMedium',
    low: 'beets.organize.recLow',
    none: 'beets.organize.recNone',
  };

  /** 证据层级（协议 v2）：说明推荐理由的强度来源。自强到弱排列，
   *  similarity 最需要人工复核。 */
  const EVIDENCE_KEY: Record<EvidenceTier, Msg> = {
    recordingId: 'beets.organize.evidenceRecordingId',
    metadataSlot: 'beets.organize.evidenceMetadataSlot',
    releaseGroup: 'beets.organize.evidenceReleaseGroup',
    similarity: 'beets.organize.evidenceSimilarity',
  };

  function fieldDiff(current: unknown, candidate: unknown): boolean {
    return String(current ?? '') !== String(candidate ?? '');
  }

  /** 当前 / 候选的专辑级字段（同名列对齐比较）。写成函数是因为 `{@const}`
   *  必须是块的直接子节点，塞不进 `<tr>` 里 */
  function curField(album: AlbumPreview, name: string): unknown {
    return (album.current as unknown as Record<string, unknown>)[name];
  }

  function candField(cand: CandidatePreview, name: string): unknown {
    return (cand.album as unknown as Record<string, unknown>)[name];
  }
</script>

<div class="beets-page">
  <!-- 待整理目录 -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>{t('beets.organize.inboxTitle')}</strong>
      <span class="beets-dim">
        {t('beets.organize.inboxDesc')}
      </span>
    </div>
    <div class="beets-row">
      {#if inbox}
        <Inbox size={14} />
        <span class="beets-path" title={inbox}>{inbox}</span>
        <Button size="sm" onclick={() => void openInbox()} title={t('beets.organize.openTip')}>
          {#snippet icon()}<FolderOpen size={13} />{/snippet}
          {t('beets.organize.open')}
        </Button>
        <Button size="sm" disabled={busy} onclick={() => void chooseInbox()}>{t('beets.organize.change')}</Button>
        <Button size="sm" variant="ghost" disabled={busy} onclick={() => void clearInbox()}>{t('beets.organize.clear')}</Button>
      {:else}
        <span class="beets-dim">{t('beets.organize.inboxEmpty')}</span>
        <Button variant="primary" size="sm" disabled={busy} onclick={() => void chooseInbox()}>
          {#snippet icon()}<Folder size={13} />{/snippet}
          {t('beets.organize.pickInbox')}
        </Button>
      {/if}
    </div>
    {#if inboxError}
      <p class="beets-danger" role="alert">{inboxError}</p>
    {/if}
  </section>

  <!-- 落点 -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>{t('beets.organize.targetTitle')}</strong>
      <span class="beets-dim">
        {t('beets.organize.targetDesc')}
      </span>
    </div>
    <div class="beets-row">
      {#if target}
        <span class="beets-path" title={target}>{target}</span>
        {#if beetsConfig.targetRoot === null}
          <span class="beets-badge" title={t('beets.organize.targetDefaultTip')}>{t('beets.organize.targetDefault')}</span>
        {/if}
      {:else}
        <span class="beets-dim">{t('beets.organize.noLibraryRoots')}</span>
      {/if}
      <Button size="sm" disabled={busy || library.roots.length === 0} onclick={() => void chooseTarget()}>
        {t('beets.organize.pickSubdir')}
      </Button>
    </div>
    {#if library.roots.length > 0}
      <div class="root-chips">
        <span class="beets-dim">{t('beets.organize.useRoots')}</span>
        {#each library.roots as root (root)}
          <Chip
            label={shortenPath(root)}
            on={beetsConfig.targetRoot === root}
            disabled={busy}
            title={root}
            onclick={() => void setTarget(root)}
          />
        {/each}
      </div>
    {/if}
    {#if targetError}
      <p class="beets-danger" role="alert">{targetError}</p>
    {/if}
  </section>

  <!-- 动作与执行 -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>{t('beets.organize.title')}</strong>
      <span class="beets-dim">
        {t('beets.organize.desc')}
      </span>
    </div>

    <div class="beets-row">
      <Button
        variant="primary"
        disabled={!inbox || busy}
        loading={running}
        onclick={startPreview}
        title={inbox ? t('beets.organize.startTip') : t('beets.organize.startBlockedTip')}
      >
        {#snippet icon()}<Play size={14} />{/snippet}
        {sess.doneCount === null ? t('beets.organize.startPreview') : t('beets.organize.repreview')}
      </Button>

      {#if !inbox}
        <span class="beets-dim">{t('beets.organize.noInboxHint')}</span>
      {/if}

      {#if running}
        <span class="beets-dim">
          {t('beets.organize.matchingPrefix')}
          {sess.progress ? `${sess.progress.current}/${sess.progress.total}` : '…'}
          {t('beets.organize.matchingTail')}
        </span>
      {/if}

      {#if busy}
        <Button size="sm" onclick={() => void sess.cancel()} ariaLabel={t('beets.organize.cancelAria')}>
          {#snippet icon()}<X size={13} />{/snippet}
          {t('common.cancel')}
        </Button>
      {/if}
    </div>

    {#if sess.doneCount !== null && !busy}
      <div class="beets-row apply-bar">
        <Select
          label={t('beets.organize.actionLabel')}
          value={action}
          options={(Object.keys(ACTION_KEY) as FileAction[]).map((value) => ({ value, label: t(ACTION_KEY[value]) }))}
          onchange={(v) => (action = v as FileAction)}
          title={t('beets.organize.actionTip')}
        />
        <Button
          variant="primary"
          size="sm"
          disabled={willApply === 0}
          loading={applying}
          onclick={() => void applyAll()}
        >
          {t('beets.organize.applyAll', { n: willApply })}
        </Button>
        {#if willApply < sess.albums.length}
          <span class="beets-dim">
            {t('beets.organize.skippedCount', { n: sess.albums.length - willApply })}
          </span>
        {/if}
        {#if applying}
          <span class="beets-dim">
            {t('beets.organize.applyingPrefix')}
            {sess.applyProgress ? `${sess.applyProgress.current}/${sess.applyProgress.total}` : '…'}
          </span>
        {/if}
      </div>
    {/if}

    {#if sess.applySummary}
      <p class="beets-dim summary" class:has-failed={sess.applySummary.failed > 0}>
        {t('beets.organize.summary', { applied: sess.applySummary.applied, skipped: sess.applySummary.skipped })}
        {#if sess.applySummary.moved > 0} · {t('beets.organize.summaryMoved', { n: sess.applySummary.moved })}{/if}
        {#if sess.applySummary.failed > 0} · {t('beets.organize.summaryFailed', { n: sess.applySummary.failed })}{/if}
        {#if (sess.applySummary.resumedSkipped ?? 0) > 0}
          · {t('beets.organize.summaryResumed', { n: sess.applySummary.resumedSkipped ?? 0 })}
        {/if}
      </p>
    {/if}

    {#if sess.lastError}
      <p class="beets-danger">
        {sess.lastError}
        <span class="beets-dim">{t('beets.organize.errorHint')}</span>
      </p>
    {/if}

    {#if sess.canResume}
      <div class="beets-row">
        <span class="beets-dim">{t('beets.organize.resumeHint')}</span>
        <Button size="sm" disabled={busy} onclick={() => void sess.resumeLast()}>
          {#snippet icon()}<RotateCcw size={13} />{/snippet}
          {t('beets.organize.resume')}
        </Button>
      </div>
    {/if}

    {#if sess.errors.length > 0}
      <div class="error-list">
        {#each sess.errors as e (e.path + e.message)}
          <div>{e.path === 'cancel' ? e.message : t('beets.organize.errorLine', { path: shortenPath(e.path), message: e.message })}</div>
        {/each}
      </div>
    {/if}

    {#if sess.albums.length === 0 && !running && sess.doneCount === null}
      <p class="beets-dim">
        {t('beets.organize.noPreview')}
      </p>
    {:else if sess.albums.length === 0 && sess.doneCount !== null}
      <p class="beets-dim">
        {sess.doneCount === 0
          ? t('beets.organize.inboxNoAudio')
          : t('beets.organize.noGroups')}
      </p>
    {/if}
  </section>

  <!-- 专辑卡 -->
  {#each sess.albums as album (album.id)}
    <section class="album-card">
      <div class="album-head">
        <b>{album.current.album || t('beets.organize.unknownAlbum')}</b>
        <span class="beets-dim">
          {album.current.artist || t('beets.organize.unknownArtist')} · {t('common.trackCount', { n: album.paths.length })}
        </span>
        <span class="beets-badge" class:accent={album.recommendation === 'strong'}>
          {REC_KEY[album.recommendation] ? t(REC_KEY[album.recommendation]) : album.recommendation}
        </span>
      </div>

      {#if sess.results[album.id]}
        <div class="apply-result" class:has-failed={sess.results[album.id].failed.length > 0}>
          <Check size={12} strokeWidth={2.4} aria-hidden="true" /> {t('beets.organize.written', { n: sess.results[album.id].written })}
          {#if sess.results[album.id].moved}· {t('beets.organize.filesHandled')}{/if}
          {#each sess.results[album.id].failed as f (f.path)}
            <div class="fail-line" class:conflict={f.code === 'conflict' || f.code === 'blankIdentity'}>
              {#if f.code === 'conflict'}<TriangleAlert size={12} strokeWidth={2.4} aria-hidden="true" /> {t('beets.organize.conflict')}{:else if f.code === 'blankIdentity'}<TriangleAlert size={12} strokeWidth={2.4} aria-hidden="true" /> {t('beets.organize.blankIdentity')}{:else}<X size={12} strokeWidth={2.4} aria-hidden="true" />{/if}
              {t('beets.organize.failLine', { path: shortenPath(f.path), message: f.message })}
            </div>
          {/each}
        </div>
      {:else if album.candidates.length === 0}
        <p class="beets-dim">{t('beets.organize.noCandidates')}</p>
      {:else}
        <Select
          label={t('beets.organize.candidateLabel')}
          value={sess.picked[album.id] ?? ''}
          disabled={applying}
          options={[
            { value: '', label: t('beets.organize.skipAlbum') },
            ...album.candidates.map((c) => ({
              value: c.id,
              label: t('beets.organize.candidateOption', {
                album: c.album.album ?? '?',
                artist: c.album.artist ?? '?',
                year: c.album.year ?? '',
                pct: (c.confidence * 100).toFixed(0),
              }),
            })),
          ]}
          onchange={(v) => (sess.picked[album.id] = v || null)}
        />

        {#if candidateOf(album)}
          {@const cand = candidateOf(album)!}
          {#if cand.evidence}
            <div class="evidence-line">
              <span class="beets-badge" class:accent={cand.evidence !== 'similarity'}>
                {EVIDENCE_KEY[cand.evidence] ? t(EVIDENCE_KEY[cand.evidence]) : cand.evidence}
              </span>
              {#each cand.reasons ?? [] as r (r)}
                <span class="beets-dim">{r}</span>
              {/each}
            </div>
          {/if}

          <table class="diff-table">
            <thead><tr><th>{t('beets.organize.diffField')}</th><th>{t('beets.organize.diffCurrent')}</th><th>{t('beets.organize.diffCandidate')}</th></tr></thead>
            <tbody>
              {#each ['album', 'artist', 'albumartist', 'year', 'genre'] as f (f)}
                <tr>
                  <td>{f}</td>
                  <td>{String(curField(album, f) ?? '')}</td>
                  <td class:changed={fieldDiff(curField(album, f), candField(cand, f))}>
                    {String(candField(cand, f) ?? '')}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>

          <details class="track-diff">
            <summary class="beets-dim">
              {t('beets.organize.trackDiffSummary', { n: Object.keys(cand.mapping).length })}
            </summary>
            {#each album.current.tracks as track (track.path)}
              <div class="track-row">
                <span class="beets-dim">{track.title || shortenPath(track.path ?? '')}</span>
                <span>{cand.mapping[track.path ?? '']?.title ?? t('beets.organize.trackUnmatched')}</span>
              </div>
            {/each}
          </details>

          <div class="album-actions">
            <Button size="sm" disabled={busy} onclick={() => void applyOne(album)}>
              {t('beets.organize.applyOne', { action: action === 'apply_move' ? t('beets.organize.actionMoveShort') : t('beets.organize.actionCopyShort') })}
            </Button>
          </div>
        {/if}
      {/if}
    </section>
  {/each}
</div>

<style>
  /* 曲库目录快捷 chip：落点大多就是某个曲库目录本身，打字/浏览一遍没必要 */
  .root-chips {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  /* 长路径上限：Chip 自身不裁文本，目录 chip 限宽后省略 */
  .root-chips :global(.k-chip),
  .root-chips :global(.k-chip-main) {
    max-width: 260px;
  }

  .root-chips :global(.k-chip-main) {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .apply-bar {
    padding-top: 2px;
  }

  .summary {
    padding: 8px 10px;
    border: 1px solid var(--accent);
    border-radius: 8px;
  }

  .summary.has-failed {
    border-color: var(--danger);
  }

  .error-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: var(--fs-base);
    color: var(--danger);
    line-height: 1.5;
  }

  .album-card {
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-2);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: var(--fs-base);
  }

  .album-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }

  .album-head b {
    font-size: var(--fs-md);
    font-weight: 600;
  }

  .evidence-line {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: var(--fs-sm);
  }

  .diff-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-sm);
  }

  .diff-table td,
  .diff-table th {
    padding: 2px 6px;
    text-align: left;
    border-bottom: 1px solid var(--line);
  }

  .diff-table th {
    color: var(--text-3);
    font-weight: 400;
  }

  .diff-table .changed {
    color: var(--accent);
    font-weight: 600;
  }

  .track-diff {
    font-size: var(--fs-sm);
  }

  .track-row {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 1px 0;
  }

  .album-actions {
    align-self: flex-start;
  }

  .apply-result {
    color: var(--accent);
    font-size: var(--fs-sm);
    line-height: 1.5;
  }

  .apply-result.has-failed {
    color: var(--text-1);
  }

  .fail-line.conflict {
    color: var(--warn);
  }
</style>
