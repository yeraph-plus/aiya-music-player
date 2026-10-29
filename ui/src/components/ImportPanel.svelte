<script lang="ts">
  import type { AlbumPreview, CandidatePreview } from '../lib/beets';
  import type { ApplyDecision } from '../lib/ipc';
  import { ipc } from '../lib/ipc';
  import { beetsSession as beets } from '../stores/beets-session.svelte';
  import { beetsConfig } from '../stores/beets-config.svelte';
  import { errorMessage } from '../lib/errors';
  import { shortenPath } from '../lib/format';
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import IconButton from './ui/IconButton.svelte';
  import Select from './ui/Select.svelte';

  /**
   * beets 导入面板（B1 预览 + B2 应用 + B3 移动/复制/取消）。
   * docs/40 十三轮起主入口是**固定的待整理目录**：文件丢进去 → 「整理入库」
   * → 专辑卡片 → 候选/逐字段 diff → 应用（写标签，可选移动/复制）→ 曲库
   * 自动刷新。选好的目录与曲库目录重叠会被后端拒绝（copy 留原件会重复入库）。
   * 「浏览其他文件夹」降级为次要入口，给临时目录的高级用法留着。
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
    apply_copy: '复制一份到整理库（原文件保留）',
    apply_move: '移动到整理库（原文件不在）',
  };

  /** 文件动作：默认只写标签（B3 移动/复制显式选择） */
  let moveMode = $state<MoveMode>('apply');

  /**
   * 证据层级徽标（协议 v2）：回答"为什么推荐这个"。顺序即强弱；
   * similarity 是兜底层，不给徽标底色（用户该对它多留个心眼）。
   */
  const EVIDENCE_LABELS: Record<string, string> = {
    recordingId: '指纹级 · Recording ID',
    metadataSlot: '双源印证',
    releaseGroup: 'Release Group',
    similarity: '仅相似度',
  };
  const evidenceLabel = (c: CandidatePreview): string | null =>
    (c.evidence && EVIDENCE_LABELS[c.evidence]) || null;

  const moveOptions: Array<{ value: string; label: string }> = Object.entries(MOVE_LABELS).map(
    ([value, label]) => ({ value, label }),
  );

  const running = $derived(beets.phase === 'previewing');
  const applying = $derived(beets.phase === 'applying');
  const inbox = $derived(beetsConfig.inbox);

  // 配置（inbox/输出根）随面板挂载拉一次；store 幂等
  $effect(() => {
    void beetsConfig.load();
  });

  /** 待整理目录行内置报错（选择/更换被后端校验拒绝时的人话提示） */
  let inboxError = $state<string | null>(null);

  /** 选择/更换待整理目录：系统对话框 → 后端校验落库（重叠会明确报错） */
  async function changeInbox(): Promise<void> {
    inboxError = null;
    let path: string | null = null;
    try {
      const chosen = await ipc.library.chooseRoot();
      path = chosen?.path ?? null;
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

  /** 打开待整理目录（往里拖文件的入口）；没设置时这里不可见 */
  async function openInbox(): Promise<void> {
    inboxError = null;
    try {
      await beetsConfig.openInbox();
    } catch (e) {
      inboxError = errorMessage(e);
    }
  }

  // store 的 syncVersion 递增 = 一次新的曲库刷新完成 → 通知主视图重拉。
  // 以挂载时的版本为基线，避免打开面板时被旧会话的结果误触发。
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
    <h2>整理入库</h2>
    {#if onclose}
      <IconButton label="关闭" size="sm" onclick={onclose}>
        <Icon name="close" size={13} />
      </IconButton>
    {/if}
  </header>

  <div class="import-body">
    <!-- 待整理目录（docs/40 十三轮）：入库流程的固定起点。文件丢进来 →
         「整理入库」→ 下面出专辑卡片。重叠校验在后端（invalid_arg 人话提示） -->
    <div class="inbox-row">
      <div class="inbox-cell">
        <span class="inbox-label">待整理目录</span>
        {#if inbox}
          <span class="inbox-path" title={inbox}>{shortenPath(inbox)}</span>
        {:else}
          <span class="dim">还没设置：选一个曲库目录之外的文件夹，把要整理的文件丢进去</span>
        {/if}
      </div>
      <div class="inbox-actions">
        {#if inbox}
          <IconButton label="打开待整理目录" size="sm" onclick={() => void openInbox()}>
            <Icon name="folder" size={13} />
          </IconButton>
          <Button size="sm" onclick={() => void changeInbox()} disabled={running || applying}>更换</Button>
        {:else}
          <Button size="sm" onclick={() => void changeInbox()} disabled={running || applying}>选择待整理目录</Button>
        {/if}
      </div>
    </div>
    {#if inboxError}
      <div class="import-error" role="alert">{inboxError}</div>
    {/if}

    <div class="import-actions">
      {#if inbox}
        <Button variant="primary" size="sm" onclick={() => void beets.previewInbox(inbox)} disabled={running || applying}>
          {#snippet icon()}<Icon name="folder" size={13} />{/snippet}
          整理入库
        </Button>
      {/if}
      <Button size="sm" onclick={() => void chooseAndPreview()} disabled={running || applying} title="临时从别的文件夹导入（不走待整理目录）">
        浏览其他文件夹
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
        <span class="mode-select">
          <Select
            label="移动/复制模式"
            value={moveMode}
            options={moveOptions}
            onchange={(v) => (moveMode = v as MoveMode)}
            title="移动/复制按 beets 的 path_formats 计算目标路径；目标已存在时该曲目不覆盖，报冲突"
          />
        </span>
        <Button variant="primary" size="sm" onclick={() => void applyAll()} disabled={applyableCount === 0}>
          应用所选（{applyableCount} 张）
        </Button>
      {/if}
    </div>

    {#if beets.applySummary}
      <div class="apply-summary" class:has-failed={beets.applySummary.failed > 0}>
        已应用 {beets.applySummary.applied} 张 · 跳过 {beets.applySummary.skipped} 张
        {#if beets.applySummary.moved > 0} · 已{MOVE_LABELS[moveMode].replace('到 beets 库', '')} {beets.applySummary.moved} 首{/if}
        {#if beets.applySummary.failed > 0} · 失败 {beets.applySummary.failed} 首{/if}
        {#if (beets.applySummary.resumedSkipped ?? 0) > 0} · 续跑跳过（已完成）{beets.applySummary.resumedSkipped} 张{/if}
      </div>
    {/if}

    {#if beets.lastError}
      <div class="import-error">
        {beets.lastError}
        <div class="dim">若提示组件缺失或版本不符，到上方「整理库」区块重装。</div>
      </div>
    {/if}

    {#if beets.canResume}
      <div class="resume-row">
        <span class="dim">上次应用有中断——已完成的专辑不会重写，只补剩余的。</span>
        <Button size="sm" onclick={() => void beets.resumeLast()} disabled={running || applying}>
          续跑上次应用
        </Button>
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
              <div class="fail-line" class:conflict={f.code === 'conflict' || f.code === 'blankIdentity'}>
                {f.code === 'conflict' ? '⚠ 目标冲突' : f.code === 'blankIdentity' ? '⚠ 身份空白' : '✗'}
                {f.path.split(/[\\/]/).pop()}：{f.message}
              </div>
            {/each}
          </div>
        {/if}

        <!-- 候选选择 -->
        {#if album.candidates.length === 0}
          <div class="dim">无候选（未联网或匹配不上）</div>
        {:else if !beets.results[album.id]}
          <Select
            label="候选匹配"
            value={beets.picked[album.id] ?? ''}
            disabled={applying}
            options={[
              { value: '', label: '保持当前标签（不修改）' },
              ...album.candidates.map((c) => ({
                value: c.id,
                label:
                  `${c.album.album ?? '?'} — ${c.album.artist ?? '?'} ${c.album.year ?? ''}` +
                  `（${(c.confidence * 100).toFixed(0)}%）`,
              })),
            ]}
            onchange={(v) => (beets.picked[album.id] = v || null)}
          />

          {#if candidateMeta(album)}
            {#if evidenceLabel(candidateMeta(album)!)}
              <div class="evidence-line">
                <span class="ev-badge" class:weak={candidateMeta(album)!.evidence === 'similarity'}>
                  {evidenceLabel(candidateMeta(album)!)}
                </span>
                {#each candidateMeta(album)!.reasons ?? [] as r (r)}
                  <span class="dim">{r}</span>
                {/each}
              </div>
            {/if}
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

  /* 待整理目录行（docs/40 十三轮）：入库流程的固定起点。
     边框盒与快捷键须知同语言——它是"先看一眼再动手"的操作前提 */
  .inbox-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 9px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-2);
  }

  .inbox-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .inbox-label {
    font-size: 11px;
    color: var(--text-3);
  }

  .inbox-path {
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .inbox-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
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
  .resume-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 12px;
  }
  /* 证据徽标（协议 v2）：similarity 层刻意弱化——它是最该被怀疑的推荐 */
  .evidence-line {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 11px;
  }
  .ev-badge {
    border: 1px solid var(--accent);
    color: var(--accent);
    border-radius: 999px;
    padding: 1px 8px;
    font-size: 10.5px;
    white-space: nowrap;
  }
  .ev-badge.weak {
    border-color: var(--line-strong, var(--line));
    color: var(--text-2);
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
  /* kit Select 的触发器自带 width:100%（专辑卡里正好占满）；
     动作行里的模式下拉不占满——外层包一层 fit-content 收缩到内容宽 */
  .mode-select {
    display: block;
    width: fit-content;
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
</style>
