<script lang="ts">
  import { onMount } from 'svelte';
  import { Download, ListFilter, Package, Search, Tag } from '@lucide/svelte';
  import Header from '../components/content/Header.svelte';
  import Button from '../components/ui/Button.svelte';
  import CheckCell from '../components/ui/CheckCell.svelte';
  import Dialog from '../components/ui/Dialog.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import Field from '../components/ui/Field.svelte';
  import SegmentedControl from '../components/ui/SegmentedControl.svelte';
  import Select from '../components/ui/Select.svelte';
  import { OP_LABELS, opsFor, beetsQuery as q } from '../stores/beets-query.svelte';
  import { ui } from '../stores/ui.svelte';
  import { shortenPath } from '../lib/format';
  import type { BeetsInstallTrack } from '../lib/ipc';

  /**
   * 整理库（beets）单页：**查询 → 存为歌单**。
   *
   * ## 为什么单独一页，而不是塞回「扫描入库」
   *
   * 这一页读的是**另一个库**（beets 自己的 `library.db`），写的是本应用曲库
   * 的歌单。两者唯一的交集是"同一个文件"这一事实。以前的实现把这两件事
   * （扫描入库 + 整理库维护）放在同一页的两段里，结果是术语打架（两个都叫
   * "库"）、状态互相牵连（扫描中不能整理）、以及一屏十几个灰按钮。
   *
   * 现在这一页**只做一件事**：用 beets 的查询语法挑一批歌，存成歌单。
   * 标签重写 / 查重 / 重排文件那些命令后端还在（`beets_update` 等），但
   * 不在这页给入口——它们是"改用户的文件"，属于另一个心智模型，等有明确的
   * 使用场景时再单独做页（docs/48 记录了这次取舍）。
   *
   * ## 三块的分工
   *
   * - **组件状态**：没装就只显示安装引导，装了就显示库根与版本，然后才是查询。
   * - **查询**：条件模式（结构化，范围可靠）/ 原始语法（beets 自己的 DSL，逃生口）。
   * - **结果**：每行标"已入库 / 未入库"。未入库的行不可选、不会进歌单。
   */

  /** 工具入口来源（docs/33 §6.1）：排障第一个问题是"为什么用的是这个" */
  function entrySourceText(source: string | null | undefined): string {
    switch (source) {
      case 'env':
        return '环境变量 AIYA_BEETS_BIN';
      case 'settings':
        return '设置里记录的路径';
      case 'path':
        return '系统 PATH';
      case 'dev':
        return '源码树（开发模式）';
      default:
        return source ?? '未知';
    }
  }

  function installTrackText(track: BeetsInstallTrack): string {
    return track === 'bundle' ? '免安装包' : '快速安装';
  }

  const found = $derived(ui.beets?.found === true);
  /** 协议指纹对不上 = 装的组件与播放器不是同一版 schema，查询会读出无意义的形状 */
  const schemaMismatch = $derived(found && ui.beets?.schemaMatched === false);

  const headerMeta = $derived.by(() => {
    if (ui.beets === null) return '正在检测整理组件…';
    if (!found) return '未启用（不影响播放）';
    if (schemaMismatch) return '组件版本不匹配';
    const parts = [`beets ${ui.beets.beetsVersion ?? '未知版本'}`];
    if (ui.beets.libraryDirectory) parts.push(`库根 ${shortenPath(ui.beets.libraryDirectory)}`);
    return parts.join(' · ');
  });

  onMount(() => {
    void (async () => {
      if (!ui.beets) await ui.refreshBeets();
      if (ui.beets?.found) await q.loadCatalog();
    })();
  });

  /** 值单元格：字段可能不存在（未回带），也可能不是字符串 */
  function cell(row: { hit: { fields: Record<string, unknown> } }, name: string): string {
    const v = row.hit.fields?.[name];
    if (v === null || v === undefined) return '';
    if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(2);
    if (typeof v === 'boolean') return v ? '是' : '否';
    return String(v);
  }
</script>

{#snippet installActions()}
  <div class="beets-install">
    <Button
      variant="primary"
      size="sm"
      onclick={() => void ui.installBeets('installer')}
      title="下载安装器（约 33KB），用系统 Python 建环境"
    >
      {#snippet icon()}<Download size={14} />{/snippet}
      快速安装
    </Button>
    <Button
      size="sm"
      onclick={() => void ui.installBeets('bundle')}
      title="下载自带运行时的套件包（约 31MB），不需要系统 Python"
    >
      {#snippet icon()}<Package size={14} />{/snippet}
      免安装包
    </Button>
    <span class="dim">
      快速安装要求机器上有 Python 3.10–3.14；没有就用免安装包（自带解释器，解压即用）。
    </span>
  </div>
{/snippet}

<div class="page-scroll beets-page">
  <Header title="整理库" big meta={headerMeta} />

  {#if ui.beets === null}
    <p class="dim">正在检测整理组件…</p>
  {:else if !found}
    <section class="panel">
      <div class="panel-head">
        {#if ui.beets.errorCode === 'E_PROTOCOL_MISMATCH'}
          <strong>整理组件版本不匹配</strong>
          <span class="dim">{ui.beets.errorMessage ?? '重装或升级即可，曲库与配置不受影响'}</span>
        {:else}
          <strong>整理库未启用</strong>
          <span class="dim">
            不影响播放。装上之后才能按标签/评分/插件字段（如 bpm、情绪）查询整理库，
            并把结果存成歌单。
          </span>
        {/if}
      </div>
      {@render installActions()}
      {#if ui.beetsInstall.running}
        <p class="dim">
          {installTrackText(ui.beetsInstall.track)} · {ui.beetsInstall.stage}
          {ui.beetsInstall.message ? `· ${ui.beetsInstall.message}` : ''}
        </p>
      {:else if ui.beetsInstall.error}
        <p class="danger">{ui.beetsInstall.error}</p>
      {/if}
    </section>
  {:else if schemaMismatch}
    <section class="panel">
      <div class="panel-head">
        <strong>整理组件与播放器的协议契约不一致</strong>
        <span class="dim">
          schema 指纹比对失败，查询会读到对不上的形状，因此停用。重装整理组件，或升级播放器后重试。
        </span>
      </div>
      {@render installActions()}
    </section>
  {:else}
    <!-- 库状态：一行。要的是"我在查哪个库"，不是一份完整诊断（那在设置→关于） -->
    <div class="lib-line">
      <span>
        库文件
        <b class="path" title={ui.beets.libraryPath ?? ''}>
          {ui.beets.libraryPath ? shortenPath(ui.beets.libraryPath) : '未知'}
        </b>
      </span>
      <span>入口 <b>{entrySourceText(ui.beets.entrySource)}</b></span>
      {#if ui.beets.pluginSummary}
        <span class="dim">
          插件 {ui.beets.pluginSummary.ready} 可用{#if ui.beets.pluginSummary.missingDeps} · {ui.beets
              .pluginSummary.missingDeps} 缺依赖{/if}
        </span>
      {/if}
      {#if ui.beets.libraryLock === 'held'}
        <span class="lock-badge" title="另一个进程在写库，查询可能读到旧数据">
          库被占用（{ui.beets.libraryLockHolder ?? '另一个进程'}）
        </span>
      {/if}
    </div>

    <!-- ---------------- 查询条件 ---------------- -->
    <section class="panel query-panel">
      <div class="panel-head">
        <strong>查询</strong>
        <span class="dim">
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
        <span class="dim">最多返回</span>
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
              <span class="dim">不加条件 = 整个整理库</span>
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
        <p class="warn-line">
          字段清单读取失败，下拉框只有基础字段：{q.catalogError}
        </p>
      {/if}

      <div class="run-row">
        <Button variant="primary" loading={q.running} onclick={() => void q.run()}>
          {#snippet icon()}<Search size={14} />{/snippet}
          查询
        </Button>
        {#if q.ran}
          <span class="dim">
            命中 {q.total} 首{#if q.truncated}（只显示前 {q.rows.length} 首，缩小条件或提高上限）{/if}
          </span>
        {/if}
      </div>

      {#if q.error}
        <p class="error-line">{q.error}</p>
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
          <span class="dim">
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
          hint="设好条件点「查询」。整理库里有什么，取决于你自己的 beets 库。"
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
                  <span class="chip-missing" title="这个文件还没被扫描进曲库，存歌单时会跳过">未入库</span>
                {/if}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
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
  .beets-page {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .panel {
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .panel-head {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .panel-head strong {
    font-size: 14px;
    font-weight: 600;
  }

  .panel-head .dim,
  .dim {
    font-size: 12px;
    line-height: 1.55;
  }

  .beets-install {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .danger {
    color: var(--danger);
    font-size: 12px;
  }

  .lib-line {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    font-size: 12px;
    color: var(--text-2);
  }

  .lib-line b {
    color: var(--text-1);
    font-weight: 500;
  }

  .path {
    font-family: var(--font-mono, ui-monospace, monospace);
  }

  .lock-badge {
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }

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

  .warn-line {
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }

  .error-line {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--danger);
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

  .chip-missing {
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 11px;
    background: var(--surface-input);
    color: var(--text-3);
    white-space: nowrap;
  }

  code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--surface-input);
  }
</style>
