<script lang="ts">
  import { Copy, DatabaseZap, FolderTree, Tags } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import { ipc } from '../lib/ipc';
  import { errorMessage } from '../lib/errors';
  import { notify } from '../stores/notify.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 曲库维护四件套（docs/29 批 D）：建库基线 / 刷新标签 / 查找重复 / 整理文件。
   *
   * 设计取舍（都是拍板过的，别在实现里悄悄改）：
   * 1. **建库基线排第一**：整条链路只有导入会写 beets 库，用户用自有扫描器
   *    入库的曲目从没进过 beets——不做这一步，后三个按钮全都没有对象。
   * 2. **查重只读**：只列组、给「定位」，应用内不删文件。删库/删盘是危险
   *    操作，交给 beets CLI 或资源管理器。
   * 3. **整理文件两段式**：先出「从哪到哪」清单（pretend），确认后才真搬。
   *    beets 的 path_formats 会把文件重排进子目录，用户必须能先看见结果。
   * 4. 目标根只显示 `beets.libraryDirectory`（config.yaml 显式配置的那一个）；
   *    没配置就禁用——绝不回落到 `~/Music`。
   */
  const maint = $derived(ui.beetsMaint);
  const found = $derived(ui.beets?.found ?? false);
  const dir = $derived(ui.beets?.libraryDirectory ?? '');
  const busy = $derived(maint.running);

  /** 明细只渲染前 N 条：几千条 DOM 会把页面拖住，而用户本来也看不完 */
  const SHOW = 20;

  const kindLabel: Record<string, string> = {
    adopt: '建库基线',
    update: '刷新标签',
    duplicates: '查找重复',
    move: '整理文件',
  };

  function shortPath(p: string): string {
    return p.length <= 52 ? p : `…${p.slice(-50)}`;
  }

  async function reveal(path: string): Promise<void> {
    try {
      const r = await ipc.beets.reveal(path);
      if (!r.revealed && r.reason) notify.showError(r.reason);
    } catch (e) {
      notify.showError(errorMessage(e));
    }
  }

  async function cancel(): Promise<void> {
    try {
      await ipc.beets.cancel();
    } catch (e) {
      notify.showError(errorMessage(e));
    }
  }
</script>

<div class="task-status maint">
  <div class="task-status-head">
    <span class="task-name">
      <DatabaseZap size={14} />
      曲库维护
    </span>
    <span class="dim">
      {found ? (dir ? `库根 ${dir}` : '库根未配置（不会整理文件）') : '需要 beets'}
    </span>
  </div>

  <div class="maint-actions">
    <Button size="sm" disabled={!found || busy} onclick={() => void ui.adoptBeets()}>
      {#snippet icon()}<DatabaseZap size={14} />{/snippet}
      建库基线
    </Button>
    <Button size="sm" disabled={!found || busy} onclick={() => void ui.updateBeets(true)}>
      {#snippet icon()}<Tags size={14} />{/snippet}
      刷新标签
    </Button>
    <Button size="sm" disabled={!found || busy} onclick={() => void ui.findDuplicates()}>
      {#snippet icon()}<Copy size={14} />{/snippet}
      查找重复
    </Button>
    <Button size="sm" disabled={!found || busy || !dir} onclick={() => void ui.moveBeets(true)}>
      {#snippet icon()}<FolderTree size={14} />{/snippet}
      整理文件
    </Button>
    {#if busy}
      <button class="link" onclick={() => void cancel()}>取消</button>
    {/if}
  </div>

  {#if busy}
    <div class="task-detail">
      {kindLabel[maint.kind] ?? '执行中'} · {maint.stage || '准备中'}
      {#if maint.total > 0}
        <span class="dim">（{maint.current}/{maint.total}）</span>
      {/if}
    </div>
  {/if}

  {#if maint.error}
    <div class="danger maint-line">{maint.error}</div>
  {:else if maint.summary}
    <div class="maint-line">
      <b>{kindLabel[maint.kind] ?? '任务'}</b>
      <span>{maint.summary}</span>
    </div>
  {/if}

  <!-- 刷新标签：字段级 diff（预览态才给「应用」，避免一键改库） -->
  {#if maint.kind === 'update' && maint.updates.length > 0}
    <ul class="maint-list">
      {#each maint.updates.slice(0, SHOW) as upd (upd.path)}
        <li>
          <span class="path" title={upd.path}>{shortPath(upd.path)}</span>
          <span class="maint-diff">
            {#each Object.entries(upd.changes) as [field, chg] (field)}
              <span class="chip">{field} {chg.from || '∅'} → {chg.to || '∅'}</span>
            {/each}
          </span>
        </li>
      {/each}
    </ul>
    {#if maint.updates.length > SHOW}
      <div class="dim">另有 {maint.updates.length - SHOW} 条未列出</div>
    {/if}
    {#if maint.preview}
      <div class="maint-actions">
        <Button size="sm" variant="primary" disabled={busy} onclick={() => void ui.updateBeets(false)}>
          应用这些修改
        </Button>
        <span class="dim">只写 beets 库，不动文件里的标签。</span>
      </div>
    {/if}
  {/if}

  <!-- 查找重复：分组 + 定位（不删文件） -->
  {#if maint.kind === 'duplicates' && maint.dupGroups.length > 0}
    <ul class="maint-list">
      {#each maint.dupGroups.slice(0, SHOW) as group (group.key.join('\u0001'))}
        <li>
          <div class="maint-group-head">
            <span class="chip">{group.key.join(' / ') || '（空）'}</span>
            <span class="dim">{group.items.length} 首</span>
          </div>
          <ul class="maint-sublist">
            {#each group.items as item (item.path)}
              <li>
                <span class="path" title={item.path}>{shortPath(item.path)}</span>
                <span class="dim">
                  {item.format} · {item.bitrate} kbps · {Math.round(item.size / 1024)} KB
                </span>
                <button class="link" onclick={() => void reveal(item.path)}>定位</button>
              </li>
            {/each}
          </ul>
        </li>
      {/each}
    </ul>
    {#if maint.dupGroups.length > SHOW}
      <div class="dim">另有 {maint.dupGroups.length - SHOW} 组未列出</div>
    {/if}
    <div class="dim">
      只列出结果，应用内不删文件。清理请用 beets CLI（<code>beet dup -d</code>）或系统资源管理器。
    </div>
  {/if}

  <!-- 整理文件：从哪到哪 + 确认 -->
  {#if maint.kind === 'move' && maint.moves.length > 0}
    <ul class="maint-list">
      {#each maint.moves.slice(0, SHOW) as move (move.path)}
        <li>
          <span class="maint-status" class:conflict={move.status === 'conflict'} class:same={move.status === 'same'}>
            {move.status === 'move' ? '移动' : move.status === 'same' ? '原地' : '冲突'}
          </span>
          <span class="path" title={move.path}>{shortPath(move.path)}</span>
          <span class="dim">→ {shortPath(move.dest)}</span>
          {#if move.reason}<span class="danger">{move.reason}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if maint.moves.length > SHOW}
      <div class="dim">另有 {maint.moves.length - SHOW} 条未列出</div>
    {/if}
    {#if maint.preview}
      <div class="maint-actions">
        <Button
          size="sm"
          variant="danger"
          disabled={busy}
          onclick={() => void ui.moveBeets(false)}
        >
          确认移动
        </Button>
        <span class="dim">
          按 beets 的 path_formats 重排文件，目标冲突的不覆盖不改名；移动后自动对齐曲库路径。
        </span>
      </div>
    {/if}
  {/if}

  {#if !found}
    <div class="dim">安装 beets 工具（上面那个「快速安装 / 免安装包」）后可用：建库基线把现有曲目登记进 beets，之后才能刷新标签/查重/整理。</div>
  {/if}
</div>

<style>
  /* 组件内自带样式：这几个类（dim/danger/chip/path）在全局只以**后代选择器**
     的形式存在（player.css 的 `.lyrics-panel header .dim` 之类），直接用在
     本组件里不会命中。复制一份比去改全局更安全——全局改会波及别处。 */
  .maint-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .maint-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .dim {
    color: var(--text-3);
    font-size: 12px;
  }

  .danger {
    color: var(--danger, #e5484d);
    font-size: 12px;
  }

  .path {
    color: var(--text-2);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11px;
  }

  .chip {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 6px;
    background: var(--surface-3);
    color: var(--text-2);
    font-size: 11px;
    font-family: var(--font-mono, ui-monospace, monospace);
  }

  .maint-list,
  .maint-sublist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .maint-list > li {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
  }

  .maint-list > li:last-child {
    border-bottom: none;
  }

  .maint-sublist > li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .maint-group-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .maint-diff {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .maint-status {
    display: inline-block;
    min-width: 34px;
    font-size: 11px;
    color: var(--text-3);
  }

  .maint-status.conflict {
    color: var(--danger, #e5484d);
  }

  .maint-status.same {
    opacity: 0.6;
  }

  .maint .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent, #4a9eff);
    font-size: 12px;
    cursor: pointer;
  }

  .maint .link:hover {
    text-decoration: underline;
  }
</style>
