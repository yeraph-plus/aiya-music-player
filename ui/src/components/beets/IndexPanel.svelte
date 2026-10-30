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
  import { OP_LABELS, opsFor, beetsQuery as q } from '../../stores/beets-query.svelte';
  import { shortenPath } from '../../lib/format';
  import { events, ipc } from '../../lib/ipc';

  /**
   * 索引子页：**索引库 → 查询 → 存为歌单**。
   *
   * ## 为什么"建索引"和"查询"必须在同一页
   *
   * 索引空着的时候，任何筛选都是 0 命中——那不是条件写错了，是索引还没建。
   * 把建索引挪到别的页（比如安装页），用户会在这一页对着 0 结果猜自己哪里写错。
   * 所以它们是同一件事的两半，且索引卡放在查询**之上**。
   *
   * ## 只入库，不分析
   *
   * 这里跑的是 `index_import`：把曲库根整批登记进索引库（`directory` = 曲库根，
   * `import.write/copy/move: no` 全钉死的只读镜像）。数值分析（bpm / 情绪）是
   * 另一条命令，要等 xtractor 接线——绑在一起的代价是"分析器没装好时连库都不建"，
   * 实测过一次，整页灰。
   */

  let indexItems = $state<number | null>(null);
  let tracksTotal = $state<number | null>(null);
  let indexRun = $state(false);
  let indexNote = $state<string | null>(null);
  let indexError = $state<string | null>(null);
  let stopIndex: (() => void) | null = null;
  /**
   * 这一轮索引会话是不是**本面板发起的**。
   *
   * 宿主在整理完成（`apply` 终态）时会自己补一次索引，走的是同一个
   * `beets:index` 通道，而且补的只是落点目录（1 s 级别）。用户没点过按钮，
   * 就不该看到「正在入库」的文案浮在上面，也不该把按钮锁住。
   *
   * 但**终态必须照常处理**：计数和查询目录要刷新——否则用户整理完切回这一页，
   * 看到的是旧的「索引库 N 条」，而查询结果还是整理前那批。订阅因此是常驻的
   * （挂载时一次，不是每次点按钮时临时建）。
   */
  let localRun = false;

  /** 索引库现在几条：`total` 是应用 limit 之前的真实命中数，拿它当条目数。 */
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
      if (localRun) indexNote = `入库 ${e.current ?? 0}/${e.total ?? 0}`;
    } else if (e.type === 'indexDone') {
      if (localRun) {
        endIndex();
        indexNote = null;
      }
      // 不管是本面板发起还是宿主补的，索引都比刚才多了东西
      void loadIndexCount();
      void q.loadCatalog();
    } else if (e.type === 'cancelled' || e.type === 'error' || e.type === 'sessionFailed') {
      if (localRun) {
        endIndex();
        indexError = e.message ?? '入库被中断';
      }
    }
  }

  async function startIndex() {
    if (indexRun) return;
    indexRun = true;
    localRun = true;
    indexError = null;
    indexNote = '正在入库…';
    try {
      await ipc.beets.indexImport();
    } catch (e) {
      endIndex();
      indexError = e instanceof Error ? e.message : String(e);
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
    if (typeof v === 'boolean') return v ? '是' : '否';
    return String(v);
  }
</script>

<div class="beets-page">
  <!-- ---------------- 曲库索引（查询的前置条件） ---------------- -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>曲库索引</strong>
      <span class="beets-dim">
        beets 对着曲库根做<b>只读</b>分析（不写标签、不搬文件），索引库是曲库的镜像。
        查询读的就是它——索引空着的时候筛选一定 0 命中。
      </span>
    </div>
    <div class="beets-row">
      {#if tracksTotal !== null}
        <span>曲库 <b>{tracksTotal}</b> 首</span>
      {/if}
      {#if indexItems !== null}
        <span>索引库 <b>{indexItems}</b> 条</span>
      {:else}
        <span class="beets-dim">索引库状态未知</span>
      {/if}
      <Button
        size="sm"
        variant="primary"
        loading={indexRun}
        disabled={indexRun}
        onclick={() => void startIndex()}
      >
        {#snippet icon()}<ListFilter size={14} />{/snippet}
        {indexItems === 0 ? '建立索引' : '重建索引'}
      </Button>
      {#if indexNote}<span class="beets-dim">{indexNote}</span>{/if}
      {#if indexError}<span class="beets-danger">{indexError}</span>{/if}
    </div>
    {#if indexItems === 0 && !indexRun}
      <p class="beets-dim">
        索引库还是空的，下面的查询不会有结果。第一次要把曲库通读一遍，几万首需要一段时间。
      </p>
    {:else if indexItems !== null && tracksTotal !== null && indexItems < tracksTotal}
      <p class="beets-dim">
        索引库比曲库少 {tracksTotal - indexItems} 首：曲库新增的文件还没登记，重建一次就好。
      </p>
    {/if}
  </section>

  <!-- ---------------- 查询条件 ---------------- -->
  <section class="beets-panel">
    <div class="beets-panel-head">
      <strong>查询</strong>
      <span class="beets-dim">
        {#if q.mode === 'rules'}
          条件之间是「并且」。范围用两条（≥ / ≤）表达——beets 的区间语法对未注册类型的
          插件字段会静默 0 命中，所以条件在这里求值。
        {:else}
          直接写 beets 的查询语法（如 <code>year:2000..2010 genre:Jazz</code>），
          语义完全跟着 beets 走。
        {/if}
      </span>
    </div>

    <div class="mode-row">
      <SegmentedControl
        label="查询方式"
        size="sm"
        value={q.mode}
        options={[
          { value: 'rules', label: '条件', title: '按字段逐条筛选' },
          { value: 'raw', label: '原始语法', title: 'beets 自己的查询语法' },
        ]}
        onchange={(v) => (q.mode = v as 'rules' | 'raw')}
      />
      <span class="beets-dim">最多返回</span>
      <Select
        label="条数上限"
        value={String(q.limit)}
        options={[
          { value: '100', label: '100 条' },
          { value: '200', label: '200 条' },
          { value: '500', label: '500 条' },
          { value: '2000', label: '2000 条' },
        ]}
        onchange={(v) => (q.limit = Number(v))}
      />
    </div>

    {#if q.mode === 'rules'}
      <div class="rules">
        {#each q.rules as rule, index (index)}
          <div class="rule-row">
            <Select
              label="字段"
              value={rule.field}
              options={q.fields.map((f) => ({
                value: f.name,
                label: f.kind === 'flexible' ? `${f.name}（插件）` : f.name,
              }))}
              onchange={(v) => q.setRuleField(index, v)}
            />
            <Select
              label="条件"
              value={rule.op}
              options={opsFor(q.fields.find((f) => f.name === rule.field)?.kind ?? 'string').map(
                (op) => ({ value: op, label: OP_LABELS[op] ?? op }),
              )}
              onchange={(v) => q.setRuleOp(index, v)}
            />
            <Field
              label="值"
              hideLabel
              value={rule.value}
              placeholder={rule.op === 'regex' ? '正则表达式' : '值'}
              oninput={(v) => q.setRuleValue(index, v)}
              onenter={() => void q.run()}
            />
            <Button variant="ghost" size="sm" onclick={() => q.removeRule(index)} title="删掉这条条件">
              删除
            </Button>
          </div>
        {/each}
        <div class="rules-actions">
          <Button size="sm" onclick={() => q.addRule()}>+ 添加条件</Button>
          {#if q.rules.length === 0}
            <span class="beets-dim">不加条件 = 整个索引库</span>
          {/if}
        </div>
      </div>
    {:else}
      <Field
        label="beets 查询语法"
        hideLabel
        mono
        value={q.raw}
        placeholder="year:2000..2010 genre:Jazz  ·  mood_happy:1"
        oninput={(v) => (q.raw = v)}
        onenter={() => void q.run()}
      />
    {/if}

    {#if q.catalogError}
      <p class="beets-warn">字段清单读取失败，下拉框只有基础字段：{q.catalogError}</p>
    {/if}

    <div class="run-row">
      <Button variant="primary" loading={q.running} onclick={() => void q.run()}>
        {#snippet icon()}<Search size={14} />{/snippet}
        查询
      </Button>
      {#if q.ran}
        <span class="beets-dim">
          命中 {q.total} 首{#if q.truncated}（只显示前 {q.rows.length} 首，缩小条件或提高上限）{/if}
        </span>
      {/if}
    </div>

    {#if q.error}
      <p class="beets-danger">{q.error}</p>
    {/if}
  </section>

  <!-- ---------------- 结果 ---------------- -->
  <section class="result-panel">
    <div class="result-bar">
      <span class="pick-all">
        <CheckCell
          checked={q.allSelected}
          indeterminate={q.selected.size > 0 && !q.allSelected}
          disabled={q.selectablePaths.length === 0}
          label="全选本页已入库的曲目"
          ontoggle={() => q.toggleAll()}
        />
        <span class="beets-dim">
          已选 {q.selected.size} 首{#if q.missingCount > 0} · 本页 {q.missingCount} 首不在曲库{/if}
        </span>
      </span>
      <Button
        variant="primary"
        size="sm"
        disabled={q.selected.size === 0}
        onclick={() => q.openSave()}
      >
        {#snippet icon()}<Tag size={14} />{/snippet}
        存为歌单
      </Button>
    </div>

    {#if !q.ran}
      <EmptyState
        title="还没有查询"
        hint="设好条件点「查询」。索引库里有什么，取决于曲库根下有多少文件被登记过。"
        fill
      >
        {#snippet icon()}<ListFilter size={26} />{/snippet}
      </EmptyState>
    {:else if q.rows.length === 0}
      <EmptyState title="没有命中" hint="换个条件试试，或切到「原始语法」用 beets 的写法。" fill />
    {:else}
      <div class="table-head">
        <span class="col-check"></span>
        <span class="col-title">标题</span>
        <span class="col-artist">艺人</span>
        <span class="col-album">专辑</span>
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
                label={`选择 ${cell(row, 'title') || row.hit.path}`}
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
                <span class="beets-badge" title="这个文件还没被扫描进曲库，存歌单时会跳过">未入库</span>
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
    title="存为歌单"
    description={`把选中的 ${q.selectedTrackIds.length} 首存成歌单。同名歌单会合并（已在里面的曲目自动跳过），不会建出第二个。`}
    onclose={() => q.closeSave()}
    autofocus={() => undefined}
  >
    <Field
      label="歌单名"
      value={q.saveName}
      placeholder="例如：2010 年代爵士"
      maxlength={60}
      oninput={(v) => (q.saveName = v)}
      onenter={() => void q.save()}
      error={q.saveError}
    />
    {#snippet actions()}
      <Button variant="ghost" onclick={() => q.closeSave()}>取消</Button>
      <Button variant="primary" loading={q.saving} onclick={() => void q.save()}>保存</Button>
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
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 16px;
    background: var(--surface-head);
    backdrop-filter: blur(10px);
    border-bottom: 1px solid var(--line);
  }

  .pick-all {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 12px;
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
    font-size: 11.5px;
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
    font-size: 12.5px;
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
    font-size: 11.5px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-input);
  }
</style>
