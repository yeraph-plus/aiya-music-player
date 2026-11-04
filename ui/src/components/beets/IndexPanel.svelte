<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { ListFilter, Search, Tag } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import CheckCell from '../ui/CheckCell.svelte';
  import Dialog from '../ui/Dialog.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Field from '../ui/Field.svelte';
  import SegmentedControl from '../ui/SegmentedControl.svelte';
  import Select from '../ui/Select.svelte';
  import { opLabel, opsFor, beetsQuery as q } from '../../stores/beets-query.svelte';
  import { shortenPath } from '../../lib/format';
  import { errorMessage } from '../../lib/errors';
  import { events, ipc } from '../../lib/ipc';
  import { t } from '../../lib/i18n.svelte';

  /**
   * 分析子页：分析库 → 查询 → 存为歌单。
   *
   * ## 建库与查询同页
   *
   * 分析库为空时任何筛选都是 0 命中——原因不是条件写错，而是分析库尚未建立。
   * 建库入口若放在别的页（例如安装页），用户会在本页对着 0 结果排查筛选表达式。
   * 两者是同一件事的两半，因此分析卡置于查询之上。
   *
   * ## 只入库，不分析
   *
   * 这里跑的是 `index_import`：把曲库根整批登记进分析库（`directory` = 曲库根，
   * `import.write/copy/move: no` 全部固定的只读镜像）。数值分析（bpm / 情绪）是
   * 另一条命令，依赖 xtractor 接线；两者绑定会使分析器缺失时连库都无法建立，
   * 页面整体不可用。
   */

  let indexItems = $state<number | null>(null);
  let tracksTotal = $state<number | null>(null);
  let indexRun = $state(false);
  let indexNote = $state<string | null>(null);
  let indexError = $state<string | null>(null);
  let stopIndex: (() => void) | null = null;
  /**
   * 本轮分析会话是否由本面板发起。
   *
   * 宿主在整理完成（`apply` 终态）时会自行补一次分析，走同一个 `beets:index`
   * 通道，且仅覆盖落点目录（1 s 级别）。未经用户点击的会话不应显示「正在入库」
   * 文案，也不应锁定按钮。
   *
   * 但终态必须照常处理：计数与查询目录都要刷新。否则整理完成切回本页时，
   * 页面上是上一次的「分析库 N 条」，查询结果也停留在整理前那批。订阅因此常驻
   * （挂载时建立一次，不在每次点击时临时创建）。
   */
  let localRun = false;

  /** 分析库现在几条：`total` 是应用 limit 之前的真实命中数，拿它当条目数。 */
  async function loadIndexCount() {
    try {
      indexItems = (await ipc.beets.query({ limit: 1 })).total;
    } catch {
      indexItems = null;
    }
    try {
      tracksTotal = (await ipc.enrichment.worklist('xtractor', false)).totalTracks;
    } catch {
      tracksTotal = null;
    }
  }

  function onIndexEvent(raw: unknown) {
    const e = raw as {
      type?: string;
      current?: number;
      total?: number;
      items?: number;
      message?: string;
    };
    if (e.type === 'progress') {
      if (localRun) indexNote = t('beets.index.noteProgress', { current: e.current ?? 0, total: e.total ?? 0 });
    } else if (e.type === 'indexDone') {
      if (localRun) {
        endIndex();
        indexNote = null;
      }
      // 不管是本面板发起还是宿主补的，分析库都比刚才多了东西
      void loadIndexCount();
      void q.loadCatalog();
    } else if (e.type === 'cancelled' || e.type === 'error' || e.type === 'sessionFailed') {
      if (localRun) {
        endIndex();
        indexError = e.message ?? t('beets.index.errorInterrupted');
      }
    }
  }

  async function startIndex() {
    if (indexRun) return;
    indexRun = true;
    localRun = true;
    indexError = null;
    indexNote = t('beets.index.noteImporting');
    try {
      await ipc.beets.indexImport();
    } catch (e) {
      endIndex();
      // 走 errorMessage 而不是 e.message：ApiError 是普通对象不是 Error 实例，
      // instanceof 判不中会显示成 [object Object]（lib/errors.ts 头注释的坑）
      indexError = errorMessage(e);
    }
  }

  function endIndex() {
    indexRun = false;
    localRun = false;
  }

  onMount(() => {
    stopIndex = events.beetsIndex(onIndexEvent);
    void (async () => {
      await Promise.all([q.loadCatalog(), loadIndexCount()]);
    })();
  });

  onDestroy(() => stopIndex?.());

  /** 值单元格：字段可能不存在（未回带），也可能不是字符串 */
  function cell(row: { hit: { fields: Record<string, unknown> } }, name: string): string {
    const v = row.hit.fields?.[name];
    if (v === null || v === undefined) return '';
    if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(2);
    if (typeof v === 'boolean') return v ? t('beets.cellYes') : t('beets.cellNo');
    return String(v);
  }
</script>

<div class="beets-page">
  <!-- 曲库分析（查询的前置条件） -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>{t('beets.index.title')}</strong>
      <span class="beets-dim">
        {t('beets.index.descHead')}<b>{t('beets.index.descReadonly')}</b>{t('beets.index.descTail')}
      </span>
    </div>
    <div class="beets-row">
      {#if tracksTotal !== null}
        <span>{t('beets.index.libraryLabel')} <b>{tracksTotal}</b> {t('beets.index.trackUnit')}</span>
      {/if}
      {#if indexItems !== null}
        <span>{t('beets.index.indexLabel')} <b>{indexItems}</b> {t('beets.index.itemUnit')}</span>
      {:else}
        <span class="beets-dim">{t('beets.index.statusUnknown')}</span>
      {/if}
      <Button
        size="sm"
        variant="primary"
        loading={indexRun}
        disabled={indexRun}
        onclick={() => void startIndex()}
      >
        {#snippet icon()}<ListFilter size={14} />{/snippet}
        {indexItems === 0 ? t('beets.index.build') : t('beets.index.rebuild')}
      </Button>
      {#if indexNote}<span class="beets-dim">{indexNote}</span>{/if}
      {#if indexError}<span class="beets-danger">{indexError}</span>{/if}
    </div>
    {#if indexItems === 0 && !indexRun}
      <p class="beets-dim">
        {t('beets.index.emptyHint')}
      </p>
    {:else if indexItems !== null && tracksTotal !== null && indexItems < tracksTotal}
      <p class="beets-dim">
        {t('beets.index.behindHint', { n: tracksTotal - indexItems })}
      </p>
    {/if}
  </section>

  <!-- 查询条件 -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>{t('beets.query.title')}</strong>
      <span class="beets-dim">
        {#if q.mode === 'rules'}
          {t('beets.query.descRules')}
          <!-- 求值在工具侧完成，不下推 beets 查询语法。beets 的区间比较对
               未注册类型的插件字段会静默 0 命中，下推等于让"有没有结果"
               取决于第三方有没有 register 类型。本说明面向
               实现维护，不进界面文案。 -->
        {:else}
          {t('beets.query.descRawHead')} <code>year:2000..2010 genre:Jazz</code>{t('beets.query.descRawTail')}
        {/if}
      </span>
    </div>

    <div class="mode-row">
      <SegmentedControl
        label={t('beets.query.modeLabel')}
        size="sm"
        value={q.mode}
        options={[
          { value: 'rules', label: t('beets.query.modeRules'), title: t('beets.query.modeRulesTip') },
          { value: 'raw', label: t('beets.query.modeRaw'), title: t('beets.query.modeRawTip') },
        ]}
        onchange={(v) => (q.mode = v as 'rules' | 'raw')}
      />
      <span class="beets-dim">{t('beets.query.limitPrefix')}</span>
      <Select
        label={t('beets.query.limitLabel')}
        value={String(q.limit)}
        options={[100, 200, 500, 2000].map((n) => ({ value: String(n), label: t('beets.query.limitOption', { n }) }))}
        onchange={(v) => (q.limit = Number(v))}
      />
    </div>

    {#if q.mode === 'rules'}
      <div class="rules">
        {#each q.rules as rule, index (index)}
          <div class="rule-row">
            <Select
              label={t('beets.query.fieldLabel')}
              value={rule.field}
              options={q.fields.map((f) => ({
                value: f.name,
                label: f.kind === 'flexible' ? t('beets.query.flexibleField', { name: f.name }) : f.name,
              }))}
              onchange={(v) => q.setRuleField(index, v)}
            />
            <Select
              label={t('beets.query.opLabel')}
              value={rule.op}
              options={opsFor(q.fields.find((f) => f.name === rule.field)?.kind ?? 'string').map(
                (op) => ({ value: op, label: opLabel(op) }),
              )}
              onchange={(v) => q.setRuleOp(index, v)}
            />
            <Field
              label={t('beets.query.valueLabel')}
              hideLabel
              value={rule.value}
              placeholder={rule.op === 'regex' ? t('beets.query.regexPh') : t('beets.query.valueLabel')}
              oninput={(v) => q.setRuleValue(index, v)}
              onenter={() => void q.run()}
            />
            <Button variant="ghost" size="sm" onclick={() => q.removeRule(index)} title={t('beets.query.removeRuleTip')}>
              {t('common.delete')}
            </Button>
          </div>
        {/each}
        <div class="rules-actions">
          <Button size="sm" onclick={() => q.addRule()}>{t('shell.playlist.addCond')}</Button>
          {#if q.rules.length === 0}
            <span class="beets-dim">{t('beets.query.noConds')}</span>
          {/if}
        </div>
      </div>
    {:else}
      <Field
        label={t('beets.query.rawLabel')}
        hideLabel
        mono
        value={q.raw}
        placeholder="year:2000..2010 genre:Jazz  ·  mood_happy:1"
        oninput={(v) => (q.raw = v)}
        onenter={() => void q.run()}
      />
    {/if}

    {#if q.catalogError}
      <p class="beets-warn">{t('beets.query.catalogError', { message: q.catalogError })}</p>
    {/if}

    <div class="run-row">
      <Button variant="primary" loading={q.running} onclick={() => void q.run()}>
        {#snippet icon()}<Search size={14} />{/snippet}
        {t('beets.query.run')}
      </Button>
      {#if q.ran}
        <span class="beets-dim">
          {t('beets.query.hits', { n: q.total })}{#if q.truncated}{t('beets.query.truncated', { n: q.rows.length })}{/if}
        </span>
      {/if}
    </div>

    {#if q.error}
      <p class="beets-danger">{q.error}</p>
    {/if}
  </section>

  <!-- 结果 -->
  <section class="result-panel">
    <div class="result-bar">
      <span class="pick-all">
        <CheckCell
          checked={q.allSelected}
          indeterminate={q.selected.size > 0 && !q.allSelected}
          disabled={q.selectablePaths.length === 0}
          label={t('beets.result.selectAll')}
          ontoggle={() => q.toggleAll()}
        />
        <span class="beets-dim">
          {t('beets.result.selected', { n: q.selected.size })}{#if q.missingCount > 0} · {t('beets.result.pageMissing', { n: q.missingCount })}{/if}
        </span>
      </span>
      <Button
        variant="primary"
        size="sm"
        disabled={q.selected.size === 0}
        onclick={() => q.openSave()}
      >
        {#snippet icon()}<Tag size={14} />{/snippet}
        {t('beets.result.savePlaylist')}
      </Button>
    </div>

    {#if !q.ran}
      <EmptyState
        title={t('beets.result.emptyTitle')}
        hint={t('beets.result.emptyHint')}
        fill
      >
        {#snippet icon()}<ListFilter size={26} />{/snippet}
      </EmptyState>
    {:else if q.rows.length === 0}
      <EmptyState title={t('beets.result.noHitsTitle')} hint={t('beets.result.noHitsHint')} fill />
    {:else}
      <div class="table-head">
        <span class="col-check"></span>
        <span class="col-title">{t('beets.result.colTitle')}</span>
        <span class="col-artist">{t('beets.result.colArtist')}</span>
        <span class="col-album">{t('beets.result.colAlbum')}</span>
        {#each q.returnFields.filter((f) => !['title', 'artist', 'album'].includes(f)) as extra (extra)}
          <span class="col-extra">{extra}</span>
        {/each}
        <span class="col-state"></span>
      </div>
      <ul class="rows">
        {#each q.rows as row (row.hit.path)}
          <li class="row" class:missing={row.trackId === null} class:on={q.isSelected(row.hit.path)}>
            <span class="col-check">
              <CheckCell
                checked={q.isSelected(row.hit.path)}
                disabled={row.trackId === null}
                label={t('beets.result.selectRow', { name: cell(row, 'title') || row.hit.path })}
                ontoggle={() => row.trackId && q.toggle(row.hit.path)}
              />
            </span>
            <span class="col-title" title={row.hit.path}>
              {cell(row, 'title') || shortenPath(row.hit.path)}
            </span>
            <span class="col-artist">{cell(row, 'artist')}</span>
            <span class="col-album">{cell(row, 'album')}</span>
            {#each q.returnFields.filter((f) => !['title', 'artist', 'album'].includes(f)) as extra (extra)}
              <span class="col-extra">{cell(row, extra)}</span>
            {/each}
            <span class="col-state">
              {#if row.trackId === null}
                <span class="beets-badge" title={t('beets.result.notIndexedTip')}>{t('beets.result.notIndexed')}</span>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

{#if q.saveOpen}
  <Dialog
    title={t('beets.result.savePlaylist')}
    description={t('beets.save.desc', { n: q.selectedTrackIds.length })}
    onclose={() => q.closeSave()}
    autofocus={() => undefined}
  >
    <Field
      label={t('beets.save.nameLabel')}
      value={q.saveName}
      placeholder={t('beets.save.namePh')}
      maxlength={60}
      oninput={(v) => (q.saveName = v)}
      onenter={() => void q.save()}
      error={q.saveError}
    />
    {#snippet actions()}
      <Button variant="ghost" onclick={() => q.closeSave()}>{t('common.cancel')}</Button>
      <Button variant="primary" loading={q.saving} onclick={() => void q.save()}>{t('common.save')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .mode-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .rules {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .rule-row {
    display: grid;
    grid-template-columns: minmax(140px, 180px) minmax(96px, 120px) minmax(160px, 1fr) auto;
    gap: 8px;
    align-items: center;
  }

  .rules-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .run-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .result-panel {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--line);
    border-radius: 12px;
    overflow: hidden;
    background: var(--surface-2);
  }

  /* 工具条吸顶：滚到第 100 行时还要能点「存为歌单」 */
  .result-bar {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 16px;
    background: var(--surface-head);
    backdrop-filter: blur(var(--blur-head));
    border-bottom: 1px solid var(--line);
  }

  .pick-all {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: var(--fs-base);
  }

  /* 表格与表头共用同一套列宽：两边各写一份的话，改列时会漏改一边，
     症状是"表头写着 bpm，数字却落在专辑列下面" */
  .table-head,
  .row {
    display: grid;
    grid-template-columns: 28px minmax(160px, 2fr) minmax(110px, 1fr) minmax(110px, 1fr);
    grid-auto-columns: minmax(56px, auto);
    grid-auto-flow: column;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
  }

  .table-head {
    height: 30px;
    font-size: var(--fs-sm);
    color: var(--text-3);
    border-bottom: 1px solid var(--line);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row {
    height: 38px;
    font-size: var(--fs-base);
    color: var(--text-1);
    border-bottom: 1px solid var(--line);
  }

  .row:last-child {
    border-bottom: 0;
  }

  .row:hover {
    background: var(--surface-hover);
  }

  .row.on {
    background: var(--surface-active);
  }

  /* 未入库：整行压暗 + 不可选。不是"禁用按钮"那种半透明假可用 */
  .row.missing {
    opacity: 0.48;
  }

  .col-check {
    display: flex;
    align-items: center;
  }

  .col-title,
  .col-artist,
  .col-album,
  .col-extra,
  .col-state {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row .col-artist,
  .row .col-album {
    color: var(--text-2);
  }

  .col-extra {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    min-width: 56px;
  }

  code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: var(--fs-sm);
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-input);
  }
</style>
