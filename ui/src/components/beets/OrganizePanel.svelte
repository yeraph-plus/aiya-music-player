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

  /**
   * 整理子页：**待整理目录 → 预览 → 落点 → 移动或拷贝**。
   *
   * ## 三个目录各管什么（docs/49 §3）
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

  const ACTION_LABELS: Record<FileAction, string> = {
    apply_copy: '复制进来（原文件保留）',
    apply_move: '移动进来（原文件不在）',
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

  /* ---------------- 待整理目录 ---------------- */

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

  /* ---------------- 落点 ---------------- */

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

  /* ---------------- 预览与应用 ---------------- */

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

  /* ---------------- 展示用 ---------------- */

  const recLabel: Record<string, string> = {
    strong: '高置信',
    medium: '中置信',
    low: '低置信',
    none: '无匹配',
  };

  /** 证据层级（协议 v2）：回答"为什么推荐这个"。自强到弱，similarity 最该被怀疑 */
  const EVIDENCE_LABELS: Record<EvidenceTier, string> = {
    recordingId: '指纹级 · Recording ID',
    metadataSlot: '双源印证',
    releaseGroup: 'Release Group',
    similarity: '仅相似度',
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
  <!-- ---------------- 待整理目录 ---------------- -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>待整理目录</strong>
      <span class="beets-dim">
        把还没进曲库的文件丢进这个文件夹，整理时从这里读。它必须在曲库目录**之外**
        ——复制模式下原件会留在这里，落在曲库里就会被扫描器再扫一遍变成重复曲目。
      </span>
    </div>
    <div class="beets-row">
      {#if inbox}
        <Inbox size={14} />
        <span class="beets-path" title={inbox}>{inbox}</span>
        <Button size="sm" onclick={() => void openInbox()} title="在资源管理器打开（往里拖文件）">
          {#snippet icon()}<FolderOpen size={13} />{/snippet}
          打开
        </Button>
        <Button size="sm" disabled={busy} onclick={() => void chooseInbox()}>更换</Button>
        <Button size="sm" variant="ghost" disabled={busy} onclick={() => void clearInbox()}>清除</Button>
      {:else}
        <span class="beets-dim">还没设置。选一个曲库之外的文件夹，把要整理的文件丢进去。</span>
        <Button variant="primary" size="sm" disabled={busy} onclick={() => void chooseInbox()}>
          {#snippet icon()}<Folder size={13} />{/snippet}
          选择待整理目录
        </Button>
      {/if}
    </div>
    {#if inboxError}
      <p class="beets-danger" role="alert">{inboxError}</p>
    {/if}
  </section>

  <!-- ---------------- 落点 ---------------- -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>整理落点</strong>
      <span class="beets-dim">
        整理后的文件按「艺人/专辑/曲序 标题」落到这里（格式跟着 beets 的 path_formats）。
        必须在曲库目录之内——落在曲库外，整理完扫描器看不见，曲库里就没有。
      </span>
    </div>
    <div class="beets-row">
      {#if target}
        <span class="beets-path" title={target}>{target}</span>
        {#if beetsConfig.targetRoot === null}
          <span class="beets-badge" title="没单独选过，跟着第一个曲库目录走">默认（第一个曲库目录）</span>
        {/if}
      {:else}
        <span class="beets-dim">还没有曲库目录，先到设置里加一个扫描根。</span>
      {/if}
      <Button size="sm" disabled={busy || library.roots.length === 0} onclick={() => void chooseTarget()}>
        选择子目录
      </Button>
    </div>
    {#if library.roots.length > 0}
      <div class="root-chips">
        <span class="beets-dim">或直接用曲库目录：</span>
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

  <!-- ---------------- 动作与执行 ---------------- -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>整理</strong>
      <span class="beets-dim">
        先预览：联网匹配候选元数据并给出逐字段差异，这一步不写任何文件。确认后才会
        写标签并按上面的动作处理文件。
      </span>
    </div>

    <div class="beets-row">
      <Button
        variant="primary"
        disabled={!inbox || busy}
        loading={running}
        onclick={startPreview}
        title={inbox ? '从待整理目录读取文件并匹配（只读）' : '先设置待整理目录'}
      >
        {#snippet icon()}<Play size={14} />{/snippet}
        {sess.doneCount === null ? '开始整理（预览）' : '重新预览'}
      </Button>

      {#if !inbox}
        <span class="beets-dim">设置待整理目录后才能开始。</span>
      {/if}

      {#if running}
        <span class="beets-dim">
          匹配中 {sess.progress ? `${sess.progress.current}/${sess.progress.total}` : '…'}
          （MusicBrainz，只读不改文件）
        </span>
      {/if}

      {#if busy}
        <Button size="sm" onclick={() => void sess.cancel()} ariaLabel="取消当前整理">
          {#snippet icon()}<X size={13} />{/snippet}
          取消
        </Button>
      {/if}
    </div>

    {#if sess.doneCount !== null && !busy}
      <div class="beets-row apply-bar">
        <Select
          label="文件动作"
          value={action}
          options={Object.entries(ACTION_LABELS).map(([value, label]) => ({ value, label }))}
          onchange={(v) => (action = v as FileAction)}
          title="移动不可逆；拷贝会在待整理目录留下原件"
        />
        <Button
          variant="primary"
          size="sm"
          disabled={willApply === 0}
          loading={applying}
          onclick={() => void applyAll()}
        >
          应用所选（{willApply} 张）
        </Button>
        {#if willApply < sess.albums.length}
          <span class="beets-dim">
            {sess.albums.length - willApply} 张没有选中候选，按跳过处理
          </span>
        {/if}
        {#if applying}
          <span class="beets-dim">
            处理中 {sess.applyProgress ? `${sess.applyProgress.current}/${sess.applyProgress.total}` : '…'}
          </span>
        {/if}
      </div>
    {/if}

    {#if sess.applySummary}
      <p class="beets-dim summary" class:has-failed={sess.applySummary.failed > 0}>
        已应用 {sess.applySummary.applied} 张 · 跳过 {sess.applySummary.skipped} 张
        {#if sess.applySummary.moved > 0} · 文件已处理 {sess.applySummary.moved} 首{/if}
        {#if sess.applySummary.failed > 0} · 失败 {sess.applySummary.failed} 首{/if}
        {#if (sess.applySummary.resumedSkipped ?? 0) > 0}
          · 续跑跳过（已完成）{sess.applySummary.resumedSkipped} 张
        {/if}
      </p>
    {/if}

    {#if sess.lastError}
      <p class="beets-danger">
        {sess.lastError}
        <span class="beets-dim">若提示组件缺失或版本不符，到「安装」页重装。</span>
      </p>
    {/if}

    {#if sess.canResume}
      <div class="beets-row">
        <span class="beets-dim">上次应用有中断——已完成的专辑不会重写，只补剩余的。</span>
        <Button size="sm" disabled={busy} onclick={() => void sess.resumeLast()}>
          {#snippet icon()}<RotateCcw size={13} />{/snippet}
          续跑上次应用
        </Button>
      </div>
    {/if}

    {#if sess.errors.length > 0}
      <div class="error-list">
        {#each sess.errors as e (e.path + e.message)}
          <div>{e.path === 'cancel' ? e.message : `${shortenPath(e.path)}：${e.message}`}</div>
        {/each}
      </div>
    {/if}

    {#if sess.albums.length === 0 && !running && sess.doneCount === null}
      <p class="beets-dim">
        还没有预览。待整理目录里放好文件，点「开始整理（预览）」。
      </p>
    {:else if sess.albums.length === 0 && sess.doneCount !== null}
      <p class="beets-dim">
        {sess.doneCount === 0
          ? '待整理目录里没有音频文件（或都被整理过了）。'
          : '预览完成，但没有可展示的专辑分组。'}
      </p>
    {/if}
  </section>

  <!-- ---------------- 专辑卡 ---------------- -->
  {#each sess.albums as album (album.id)}
    <section class="album-card">
      <div class="album-head">
        <b>{album.current.album || '(未知专辑)'}</b>
        <span class="beets-dim">
          {album.current.artist || '(未知艺人)'} · {album.paths.length} 首
        </span>
        <span class="beets-badge" class:accent={album.recommendation === 'strong'}>
          {recLabel[album.recommendation] ?? album.recommendation}
        </span>
      </div>

      {#if sess.results[album.id]}
        <div class="apply-result" class:has-failed={sess.results[album.id].failed.length > 0}>
          <Check size={12} strokeWidth={2.4} aria-hidden="true" /> 已写入 {sess.results[album.id].written} 首
          {#if sess.results[album.id].moved}· 文件已按动作处理{/if}
          {#each sess.results[album.id].failed as f (f.path)}
            <div class="fail-line" class:conflict={f.code === 'conflict' || f.code === 'blankIdentity'}>
              {#if f.code === 'conflict'}<TriangleAlert size={12} strokeWidth={2.4} aria-hidden="true" /> 目标冲突{:else if f.code === 'blankIdentity'}<TriangleAlert size={12} strokeWidth={2.4} aria-hidden="true" /> 身份空白{:else}<X size={12} strokeWidth={2.4} aria-hidden="true" />{/if}
              {shortenPath(f.path)}：{f.message}
            </div>
          {/each}
        </div>
      {:else if album.candidates.length === 0}
        <p class="beets-dim">无候选（没联网，或这些文件匹配不上）。</p>
      {:else}
        <Select
          label="候选匹配"
          value={sess.picked[album.id] ?? ''}
          disabled={applying}
          options={[
            { value: '', label: '跳过这张（保持当前标签，不写入）' },
            ...album.candidates.map((c) => ({
              value: c.id,
              label:
                `${c.album.album ?? '?'} — ${c.album.artist ?? '?'} ${c.album.year ?? ''}` +
                `（${(c.confidence * 100).toFixed(0)}%）`,
            })),
          ]}
          onchange={(v) => (sess.picked[album.id] = v || null)}
        />

        {#if candidateOf(album)}
          {@const cand = candidateOf(album)!}
          {#if cand.evidence}
            <div class="evidence-line">
              <span class="beets-badge" class:accent={cand.evidence !== 'similarity'}>
                {EVIDENCE_LABELS[cand.evidence] ?? cand.evidence}
              </span>
              {#each cand.reasons ?? [] as r (r)}
                <span class="beets-dim">{r}</span>
              {/each}
            </div>
          {/if}

          <table class="diff-table">
            <thead><tr><th>字段</th><th>当前</th><th>候选</th></tr></thead>
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
              逐曲目（{Object.keys(cand.mapping).length} 首对齐）
            </summary>
            {#each album.current.tracks as t (t.path)}
              <div class="track-row">
                <span class="beets-dim">{t.title || shortenPath(t.path ?? '')}</span>
                <span>{cand.mapping[t.path ?? '']?.title ?? '(对不上)'}</span>
              </div>
            {/each}
          </details>

          <div class="album-actions">
            <Button size="sm" disabled={busy} onclick={() => void applyOne(album)}>
              只应用这张（{action === 'apply_move' ? '移动' : '拷贝'}）
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

  /* 长路径上限：Chip 自身不裁文本，目录 chip 限宽后省略（视觉与旧 .root-chip 一致） */
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
