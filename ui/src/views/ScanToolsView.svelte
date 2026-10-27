<script lang="ts">
  import { RefreshCw } from '@lucide/svelte';
  import LibraryTools from '../components/LibraryTools.svelte';
  import Icon from '../components/Icon.svelte';
  import Button from '../components/ui/Button.svelte';
  import Header from '../components/content/Header.svelte';
  import { formatBytes, formatDateTime, formatDurationLong, shortenPath } from '../lib/format';
  import { findConflicts, formatBinding, resolveShortcutBindings, SHORTCUT_LABELS, type ShortcutAction } from '../lib/shortcuts';
  import { history } from '../stores/history.svelte';
  import { list } from '../stores/list.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 扫描入库页（docs/29 批 A）：扫描 / beets 任务 / 多目录 / 日志诊断。
   *
   * 从设置页拆出的理由（用户拍板）：扫描与 beets 任务是**长时运行**的，
   * 埋在设置页里意味着"开任务 → 关设置页 → 任务进度看不见"；升一级路由
   * 后任务页可以停留、刷新，侧栏高亮给出稳定位置。
   *
   * 诊断块整段搬自 SettingsPanel（「音乐库与任务」与「诊断」两个分区撤出
   * 设置页）。数据源不变：`ui.diagnostics` / `ui.beets`，进入页面时由
   * `nav.apply` 触发 `refreshDiagnostics`（与设置页同一触发点）。
   */
  const diagnostics = $derived(ui.diagnostics);
  const beets = $derived(ui.beets);
  const listTimings = $derived(diagnostics?.listTimings ?? []);
  const recentErrors = $derived(diagnostics?.recentErrors ?? []);

  // 快捷键冲突块沿用设置页的数据源（settings.shortcuts），只读展示
  const bindings = $derived(resolveShortcutBindings(ui.settings.shortcuts));
  const conflicts = $derived(findConflicts(bindings));
</script>

<div class="page-scroll tools-page">
  <Header kicker="LIBRARY TOOLS" title="扫描入库" />

  <section class="tools-section" id="tools-scan">
    <div class="section-heading"><div><h2>音乐库与任务</h2></div></div>
    <LibraryTools />
  </section>

  <section class="tools-section" id="tools-diagnostics">
    <div class="section-heading">
      <div><h2>诊断与日志</h2></div>
      <div class="diag-actions">
        <Button size="sm" onclick={() => void ui.refreshDiagnostics()}>
          {#snippet icon()}<Icon name="refresh" size={13} />{/snippet}
          刷新
        </Button>
        <Button size="sm" onclick={() => ui.openDataDir()}>打开数据目录</Button>
        <Button variant="danger" size="sm" onclick={() => void history.clear()}>清空播放历史</Button>
      </div>
    </div>

    {#if diagnostics}
      <div class="diag-grid">
        <div><span>版本</span><b>{diagnostics.version}</b></div>
        <div><span>平台</span><b>{diagnostics.platform}</b></div>
        <div><span>Schema</span><b>v{diagnostics.schemaVersion}</b></div>
        <div><span>曲库</span><b>{diagnostics.tracks} 首</b></div>
        <div><span>时长</span><b>{formatDurationLong(diagnostics.totalDurationMs)}</b></div>
        <div><span>专辑 / 艺人</span><b>{diagnostics.albums} / {diagnostics.artists}</b></div>
        <div><span>播放列表</span><b>{diagnostics.playlists}</b></div>
        <div><span>历史记录</span><b>{diagnostics.historyRows}</b></div>
        <div><span>数据库</span><b>{formatBytes(diagnostics.dbBytes)}</b></div>
        <div class="wide"><span>数据目录</span><b class="path" title={diagnostics.dataDir}>{shortenPath(diagnostics.dataDir)}</b></div>
      </div>

      <div class="diag-block">
        <div class="diag-head">
          媒体流
          <span class:danger={diagnostics.streams.active !== 0}>
            打开 {diagnostics.streams.opened} / 关闭 {diagnostics.streams.closed} / 未关闭 {diagnostics.streams.active}
          </span>
        </div>
        <div class="dim">连续切歌后"未关闭"应始终为 0；不为 0 说明协议层有泄漏。</div>
      </div>

      <div class="diag-block">
        <div class="diag-head">
          系统媒体控制
          <span>
            {#if !diagnostics.mediaKeys.enabled}
              已关闭
            {:else if diagnostics.mediaKeys.active.length > 0}
              已启用
            {:else}
              未生效
            {/if}
          </span>
        </div>
        <div class="dim">
          标题、封面与播放状态会同步到 Windows 媒体面板 / 锁屏 / 蓝牙耳机；
          只在播放时出现会话。
        </div>
      </div>

      <div class="diag-block">
        <div class="diag-head">
          系统集成
          <span>防休眠 {diagnostics.power.active ? '已拦截' : '未拦截'} · 托盘 {diagnostics.tray ? '已创建' : '未创建'}</span>
        </div>
        <div class="dim">
          播放中不应休眠（允许熄屏）；托盘图标只在"关闭窗口时最小化到托盘"打开时存在。
        </div>
      </div>

      <div class="diag-block">
        <div class="diag-head">
          beets 集成
          {#if beets === null}
            <span class="dim">探测中…</span>
          {:else if beets.found}
            <span>beets {beets.beetsVersion} · Python {beets.pythonVersion}</span>
          {:else}
            <span class="danger">未找到 beets</span>
          {/if}
        </div>
        {#if beets?.found}
          <div class="diag-grid">
            <div class="wide"><span>曲库</span><b class="path" title={beets.libraryPath ?? ''}>{beets.libraryExists ? shortenPath(beets.libraryPath ?? '') : '尚未创建（首次导入时生成）'}</b></div>
            <div class="wide"><span>入口</span><b>{beets.entrySource ?? '未知'}</b></div>
            <div class="wide"><span>配置目录</span><b class="path" title={beets.configDir ?? ''}>{shortenPath(beets.configDir ?? '')}</b></div>
          </div>
          {#if beets.pluginHealth?.length}
            <div class="plugin-list">
              {#each beets.pluginHealth as p (p.name)}
                <div class="plugin-row" data-state={p.state}>
                  <b>{p.name}</b>
                  <span class="plugin-state">
                    {p.state === 'ready' ? '可用' : p.state === 'missing_deps' ? '缺依赖' : '不可用'}
                  </span>
                  {#if p.missing.length}<span class="dim">缺 {p.missing.join('、')}</span>{/if}
                  {#if p.note}<span class="dim">{p.note}</span>{/if}
                </div>
              {/each}
            </div>
          {:else if beets.plugins?.length}
            <div class="dim">配置里启用的插件：{beets.plugins.join(', ')}（工具未提供自检结果）</div>
          {/if}
          <div class="dim">标签匹配/候选/diff 由 beets 完成（写权威）；未安装时播放功能不受影响。</div>
        {:else if beets}
          <div class="dim">
            还没装 beets 工具。到「扫描入库」页点「快速安装」（需系统 Python 3.10–3.14）
            或「免安装包」（自带运行时，不依赖系统 Python）。
            {#if beets.errorMessage}
              <br /><span class="danger">{beets.errorMessage}</span>
            {/if}
          </div>
        {/if}
      </div>

      <div class="diag-block">
        <div class="diag-head">最近查询<span class="dim">越多越慢时说明该看索引了</span></div>
        {#if listTimings.length === 0}
          <div class="dim">还没有记录</div>
        {:else}
          <table class="diag-table">
            <thead><tr><th>时间</th><th>来源</th><th>行数</th><th>总数</th><th>命令耗时</th><th>SQL</th></tr></thead>
            <tbody>
              {#each listTimings.slice(-8).reverse() as timing}
                <tr>
                  <td>{formatDateTime(timing.atMs)}</td>
                  <td>{timing.source}</td>
                  <td>{timing.rows}</td>
                  <td>{timing.total}</td>
                  <td class:slow={timing.tookMs > 50}>{timing.tookMs.toFixed(1)}ms</td>
                  <td class:slow={timing.dbTookMs > 50}>{timing.dbTookMs.toFixed(1)}ms</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

      <div class="diag-block">
        <div class="diag-head">
          快捷键冲突
          {#if conflicts.length === 0}
            <span class="dim">无</span>
          {:else}
            <span class="danger">{conflicts.length} 组</span>
          {/if}
        </div>
        {#if conflicts.length === 0}
          <div class="dim">同一组按键绑了多个动作时会出现在这里，到「设置 → 快捷键」可以改。</div>
        {:else}
          <ul class="error-list">
            {#each conflicts as conflict (conflict.binding)}
              <li>
                <b>{formatBinding(conflict.binding)}</b>
                {conflict.actions.map((action) => SHORTCUT_LABELS[action as ShortcutAction]).join('、')}
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="diag-block">
        <div class="diag-head">最近错误<span class="dim">最多保留 30 条，新的在上</span></div>
        {#if recentErrors.length === 0}
          <div class="dim">没有错误</div>
        {:else}
          <ul class="error-list">
            {#each recentErrors.slice().reverse() as item}
              <li>
                <span class="dim">{formatDateTime(item.atMs)}</span>
                <span class="danger">{item.code}</span>
                {item.message}
                {#if item.detail}
                  <span class="dim" title={item.detail}>（有详情）</span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else}
      <div class="dim"><RefreshCw size={14} class="spinning" /> 正在读取…</div>
    {/if}
  </section>
</div>

<style>
  /* 诊断值/路径/beets 提示/错误详情是用户可能要复制的内容，
     从全局 user-select:none 里豁免（原 SettingsPanel 同款约定，diag 样式
     本体在全局 overlays.css，这里不复制一份） */
  .diag-grid,
  .diag-block .dim,
  .error-list {
    user-select: text;
    cursor: text;
  }

  .tools-section {
    margin-top: 26px;
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .section-heading h2 {
    font-size: 15px;
    font-weight: 600;
  }
</style>
