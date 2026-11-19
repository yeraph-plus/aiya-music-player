<script lang="ts">
  import { Checkbox } from 'bits-ui';
  import AlertDialog from '../ui/AlertDialog.svelte';
  import Progress from '../ui/Progress.svelte';
  import { Check } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { Download, Package, RefreshCw, Trash2, TriangleAlert } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import { t, type Msg } from '../../lib/i18n.svelte';
  import { ui } from '../../stores/ui.svelte';

  /**
   * 安装子页：Beets 装没装、装的是哪一版、能不能干活（单轨）。
   *
   * 唯一的分发物是**预配置环境**（便携包）：自带解释器、pip 与预置插件
   * （内置可用的四件 + 分析器全家桶），下载 → 校验 → 解压全自动。
   *
   * 未安装 = 一张卡一个按钮；已安装 = 一张状态卡三个动作：
   * - **重新安装**：重新下载并解压（Rust 直连）——beets 无更新路径，只有
   *   安装与卸载，重装即全新状态（后装插件不保留，勾选面从 settings 恢复）；
   * - **换插件**：包内解释器重跑 `install --plugins <勾选>`——pip 装/卸、
   *   外部工具补齐、配置同步全在脚本侧（宿主只做操作桥）；
   * - **删除组件**：组件目录整体移除（Rust 直删，两段确认）。
   *
   * 依赖自检与"另下 X"提示已删：装之前替用户操心缺什么，不如让安装脚本
   * 自己说清楚。
   */

  const found = $derived(ui.beets?.found === true);
  const mismatch = $derived(found && ui.beets?.schemaMatched === false);
  const install = $derived(ui.beetsInstall);
  const uninstall = $derived(ui.beetsUninstall);
  const table = $derived(ui.beetsPlugins);
  const bundled = $derived(table.filter((p) => p.bundle));

  /** 勾选（`null` = 还没动过，用表里的默认清单） */
  let picked = $state<string[] | null>(null);
  const chosen = $derived(picked ?? table.filter((p) => p.default).map((p) => p.name));
  let confirming = $state(false);
  let cancelDelete = $state<HTMLButtonElement | null>(null);
  const busy = $derived(install.running || uninstall.running);

  onMount(() => {
    // 内嵌插件表在二进制里，不依赖"已装组件"
    void ui.loadBeetsPlugins();
  });

  /** 勾选（`null` = 还没动过，用表里的默认清单）。
   *  required 条目不参与勾选：config 渲染时工具侧的 REQUIRED_PLUGINS 并集
   *  会把它们恒并回启用行，UI 置灰而不是"勾了又被改回"——勾选态必须与
   *  config 实际启用态一致。 */
  function toggle(name: string): void {
    if (busy) return;
    if (table.find((p) => p.name === name)?.required) return;
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    picked = [...next];
  }

  /** 安装阶段徽章文本：宿主下发的 stage 取值映射为面向用户的短语；未知取值原样显示
   *  （pip 各语言同形，不进键表，走原样回落） */
  const STAGE_KEY: Record<string, Msg> = {
    prepare: 'beets.stage.prepare',
    download: 'beets.stage.download',
    install: 'beets.stage.install',
    unpack: 'beets.stage.unpack',
    bin: 'beets.stage.bin',
  };
  const stageLabel = (s: string): string => (STAGE_KEY[s] ? t(STAGE_KEY[s]) : s);

  /** 入口来源：排障第一个问题是"为什么用的是这个" */
  function entryText(source: string | null | undefined): string {
    switch (source) {
      case 'env':
        return t('beets.entry.env');
      case 'settings':
        return t('beets.entry.settings');
      case 'path':
        return t('beets.entry.path');
      default:
        return source ?? t('beets.entry.unknown');
    }
  }
</script>

<!-- 卡内安装状态：跟在按钮正下方——阶段是徽章，消息是文本 -->
{#snippet status()}
  {#if install.running}
    <Progress value={null} label={t('beets.downloadInstall')} size="md" />
    <p class="track-status" role="status">
      <span class="beets-badge">{stageLabel(install.stage)}</span>
      <span>{install.message}</span>
    </p>
  {:else if install.error}
    <p class="track-status track-status-error" role="alert">{install.error}</p>
  {/if}
{/snippet}

{#snippet uninstallLine()}
  {#if uninstall.running}
    <p class="beets-dim">{uninstall.message || t('beets.uninstallRunning')}</p>
  {:else if uninstall.error}
    <p class="beets-danger" role="alert">{uninstall.error}</p>
  {:else if uninstall.message}
    <p class="beets-dim">{uninstall.message}</p>
  {/if}
{/snippet}

{#snippet pluginPicker(hint: string)}
  <p class="beets-dim">{hint}</p>
  {#if table.length === 0}
    <p class="beets-dim">{t('beets.pluginTableEmpty')}</p>
  {:else}
    <ul class="pick">
      {#each table as p (p.name)}
        <li>
          <label>
            <Checkbox.Root class="plugin-check" aria-label={p.title} checked={chosen.includes(p.name)}
              disabled={p.required || busy} onCheckedChange={() => toggle(p.name)}
              title={p.required ? t('beets.install.requiredTip') : undefined}>
              {#snippet children({ checked })}{#if checked}<Check size={12} />{/if}{/snippet}
            </Checkbox.Root>
            <span class="pick-name">{p.name}</span>
            <span class="pick-title">{p.title}</span>
            {#if p.packages.length > 0}
              <span class="pick-tag">{p.packages.join(' ')}</span>
            {/if}
            {#if p.required}
              <span class="pick-tag">{t('beets.install.requiredTip')}</span>
            {/if}
          </label>
          <span class="pick-note">{p.note}</span>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

<div class="beets-page">
  {#if ui.beets === null}
    <section class="beets-panel">
      <p class="beets-dim">{t('beets.detectingBeets')}</p>
    </section>
  {:else if !found}
    {#if ui.beets.errorCode === 'E_PROTOCOL_MISMATCH'}
      <!-- 版本不匹配要在安装页说清原因；未安装则页头 meta 已说明，这里不再重复一行 -->
      <p class="install-note beets-danger">
        {t('beets.protocolNote', { message: ui.beets.errorMessage ?? t('beets.schemaMismatchDefault') })}
      </p>
    {/if}

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>{t('beets.bundleTitle')}</strong>
        <span class="beets-dim">
          {t('beets.bundleDesc', {
            plugins: bundled.length > 0 ? bundled.map((p) => p.title).join(' / ') : t('beets.bundleDescFallback'),
          })}
        </span>
      </div>
      <div class="beets-gate">
        <Button
          variant="primary"
          size="sm"
          loading={install.running}
          disabled={busy}
          onclick={() => void ui.installBeets()}
          title={t('beets.downloadInstall')}
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          {t('beets.downloadInstall')}
        </Button>
      </div>
      {@render status()}
    </section>
  {:else}
    {#if mismatch}
      <section class="beets-panel">
        <div class="beets-panel-head">
          <strong class="warn-title">
            <TriangleAlert size={15} /> {t('beets.contractTitle')}
          </strong>
          <span class="beets-dim">
            {t('beets.contractDesc')}
          </span>
        </div>
        <div class="beets-gate">
          <Button
            variant="primary"
            size="sm"
            loading={install.running}
            disabled={busy}
            onclick={() => void ui.installBeets()}
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            {t('beets.reinstall')}
          </Button>
        </div>
        {@render status()}
      </section>
    {/if}

    <!-- 装后状态卡：一张卡，重新安装 / 换插件 / 删除三个动作 -->
    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>{t('beets.bundleTitle')}</strong>
        <span class="beets-dim">
          {t('beets.bundleDescInstalled')}
        </span>
      </div>
      <div class="beets-row">
        <span>beets <b>{ui.beets.beetsVersion ?? t('beets.meta.versionUnknown')}</b></span>
        {#if ui.beets.pythonVersion}
          <span>Python <b>{ui.beets.pythonVersion}</b></span>
        {/if}
        {#if ui.beets.protocolVersion !== null && ui.beets.protocolVersion !== undefined}
          <span>{t('beets.protocolLabel')} <b>v{ui.beets.protocolVersion}</b></span>
        {/if}
        {#if ui.beets.libraryLock === 'held'}
          <span class="beets-badge warn" title={t('beets.libraryLockTip')}>
            {t('beets.libraryLock', { holder: ui.beets.libraryLockHolder ?? t('beets.libraryLockFallback') })}
          </span>
        {/if}
        <Button size="sm" disabled={busy} onclick={() => void ui.refreshBeets()} title={t('beets.redetectTip')}>
          {#snippet icon()}<RefreshCw size={13} />{/snippet}
          {t('beets.redetect')}
        </Button>
      </div>

      <table class="facts">
        <tbody>
          <tr>
            <th>{t('beets.factsEntry')}</th>
            <td>{entryText(ui.beets.entrySource)}</td>
          </tr>
          {#if ui.beets.interpreter}
            <tr>
              <th>{t('beets.factsInterpreter')}</th>
              <td class="mono" title={ui.beets.interpreter}>{ui.beets.interpreter}</td>
            </tr>
          {/if}
          {#if ui.beets.configDir}
            <tr>
              <th>{t('beets.factsConfig')}</th>
              <td class="mono" title={ui.beets.configDir}>{ui.beets.configDir}</td>
            </tr>
          {/if}
          {#if ui.beets.dataHome}
            <tr>
              <th>{t('beets.factsData')}</th>
              <td class="mono" title={ui.beets.dataHome}>{ui.beets.dataHome}</td>
            </tr>
          {/if}
        </tbody>
      </table>

      {@render pluginPicker(t('beets.pluginPickerHint'))}
      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running}
          disabled={busy}
          onclick={() => void ui.setBeetsPlugins(chosen)}
          title={t('beets.changePluginsTip')}
        >
          {#snippet icon()}<Download size={14} />{/snippet}
          {t('beets.changePlugins', { n: chosen.length })}
        </Button>
      </div>
      {@render status()}

      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running}
          disabled={busy}
          onclick={() => void ui.installBeets()}
          title={t('beets.updateLatestTip')}
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          {t('beets.updateLatest')}
        </Button>
      </div>

      <div class="beets-gate">
        <Button size="sm" disabled={busy} onclick={() => confirming = true}>
          {#snippet icon()}<Trash2 size={14} />{/snippet}{t('beets.deleteBundle')}
        </Button>
      </div>
      <p class="install-note">
        {t('beets.deleteNoteHead')}<b>{t('beets.deleteNoteKept')}</b>{t('beets.deleteNoteTail')}
      </p>
      {@render uninstallLine()}
    </section>
  {/if}
</div>

{#if confirming}
  <AlertDialog title={t('beets.deleteBundle')}
    description={t('beets.deleteNoteHead') + t('beets.deleteNoteKept') + t('beets.deleteNoteTail')}
    autofocus={() => cancelDelete?.focus()} onclose={() => confirming = false}>
    {#snippet actions()}
      <Button bind:ref={cancelDelete} onclick={() => confirming = false}>{t('common.cancel')}</Button>
      <Button variant="danger" onclick={() => { confirming = false; void ui.uninstallBeets(); }}>{t('beets.confirmDelete')}</Button>
    {/snippet}
  </AlertDialog>
{/if}

<style>
  /* 状态行：裸文本不进卡，与安装卡同列间距 */
  .install-note {
    margin: 0;
  }

  /* 卡内安装状态：阶段徽章 + 消息文本，跟在按钮正下方；报错换警示色 */
  .track-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-3);
  }

  .track-status-error {
    color: var(--danger);
  }

  /* 勾选清单：每项一行，勾选框 + 名字 + 中文名 + 标签 */
  .pick {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .pick li {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 3px 0;
  }

  .pick label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: var(--fs-md);
    cursor: pointer;
  }

  :global(.plugin-check) {display:inline-grid;place-items:center;flex:none;width:1.125rem;height:1.125rem;border:1px solid var(--line-strong);border-radius:var(--radius-xs);background:var(--surface-input);color:var(--accent-fg);}
  :global(.plugin-check[data-state='checked']) {background:var(--accent);border-color:var(--accent);}
  :global(.plugin-check:disabled) {opacity:var(--disabled-opacity);}
  :global(.plugin-check:focus-visible) {outline:2px solid var(--accent);outline-offset:2px;}

  .pick-name {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
  }

  .pick-title {
    color: var(--text-2);
  }

  .pick-tag {
    padding: 0 5px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--line);
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-xs);
    color: var(--text-3);
  }

  .pick-note {
    padding-left: 20px;
    font-size: var(--fs-sm);
    color: var(--text-3);
  }

  /* 状态表：左列窄、右列吃掉剩余宽度并单行省略。
     用 table 而不是 grid 是因为行数随字段有无变化（有的工具不报 pythonVersion） */
  .facts {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: var(--fs-md);
  }

  .facts th {
    text-align: left;
    font-weight: 400;
    color: var(--text-3);
    width: 6.5rem;
    padding: 3px 0;
    vertical-align: top;
  }

  .facts td {
    padding: 3px 0;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .facts .mono {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .warn-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--warn);
  }
</style>
