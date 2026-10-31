<script lang="ts">
  import { onMount } from 'svelte';
  import { Download, Package, RefreshCw, Trash2, TriangleAlert } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import { ui } from '../../stores/ui.svelte';
  import type { BeetsInstallTrack } from '../../lib/ipc';

  /**
   * 安装子页：Beets 装没装、装的是哪一版、能不能干活。
   *
   * 未安装时是**两张平级卡、各占一行**（2026-10-31 用户拍板，不卡片套卡片）：
   * - **预配置环境**（主推）：下载约 34MB，自带解释器 + 固定的基础插件清单，
   *   不需要任何前置；
   * - **本机安装**：只在探测到系统 Python 时出现——用本机 Python 建 venv，
   *   插件按勾选表装（只有这条路能改插件清单），支持 ffmpeg 等外部工具的
   *   高级功能；缺的依赖与工具脚本自己下载（docs/53）。
   *
   * 状态说明是一行裸文本，不进卡；依赖自检与"另下 X"提示已删：装之前替用户
   * 操心缺什么，不如让安装脚本自己说清楚。
   * 已安装态只剩三块：组件状态、重装/换插件、卸载。
   */

  const found = $derived(ui.beets?.found === true);
  const mismatch = $derived(found && ui.beets?.schemaMatched === false);
  const install = $derived(ui.beetsInstall);
  const uninstall = $derived(ui.beetsUninstall);
  const python = $derived(ui.beetsPython);
  const table = $derived(ui.beetsPlugins);
  const bundled = $derived(table.filter((p) => p.bundle));

  /** 勾选（`null` = 还没动过，用表里的默认清单） */
  let picked = $state<string[] | null>(null);
  const chosen = $derived(picked ?? table.filter((p) => p.default).map((p) => p.name));
  let confirming = $state(false);

  onMount(() => {
    // 两条都不依赖"已装组件"：内嵌表在二进制里，Python 探测只看系统
    void ui.loadBeetsPlugins();
    void ui.probeBeetsPython();
  });

  function toggle(name: string): void {
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    picked = [...next];
  }

  function trackText(track: BeetsInstallTrack): string {
    return track === 'bundle' ? '预配置环境' : '本机安装';
  }

  /** 入口来源（docs/33 §6.1）：排障第一个问题是"为什么用的是这个" */
  function entryText(source: string | null | undefined): string {
    switch (source) {
      case 'env':
        return '环境变量 AIYA_BEETS_BIN';
      case 'settings':
        return '设置里记录的路径';
      case 'path':
        return '系统 PATH';
      default:
        return source ?? '未知';
    }
  }
</script>

{#snippet progressLine()}
  {#if install.running}
    <p class="beets-dim">{trackText(install.track)} · {install.stage} · {install.message}</p>
  {:else if install.error}
    <p class="beets-danger">{install.error}</p>
  {/if}
{/snippet}

<!-- 卡内安装状态：只报自己这一轨，跟在按钮正下方（下载/校验/解压阶段与报错都落在动作位上） -->
{#snippet trackStatus(track: BeetsInstallTrack)}
  {#if install.running && install.track === track}
    <p class="track-status">{install.stage} · {install.message}</p>
  {:else if install.error && install.track === track}
    <p class="track-status track-status-error">{install.error}</p>
  {/if}
{/snippet}

{#snippet uninstallLine()}
  {#if uninstall.running}
    <p class="beets-dim">{uninstall.message || '正在卸载…'}</p>
  {:else if uninstall.error}
    <p class="beets-danger">{uninstall.error}</p>
  {:else if uninstall.message}
    <p class="beets-dim">{uninstall.message}</p>
  {/if}
{/snippet}

{#snippet pluginPicker(hint: string)}
  <p class="beets-dim">{hint}</p>
  {#if table.length === 0}
    <p class="beets-dim">插件表没加载出来（这不影响装基础组件）。</p>
  {:else}
    <ul class="pick">
      {#each table as p (p.name)}
        <li>
          <label>
            <input type="checkbox" checked={chosen.includes(p.name)} onchange={() => toggle(p.name)} />
            <span class="pick-name">{p.name}</span>
            <span class="pick-title">{p.title}</span>
            {#if p.packages.length > 0}
              <span class="pick-tag">{p.packages.join(' ')}</span>
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
      <p class="beets-dim">正在检测 Beets…</p>
    </section>
  {:else if !found}
    {#if ui.beets.errorCode === 'E_PROTOCOL_MISMATCH'}
      <!-- 版本不匹配要在安装页说清原因；未安装则页头 meta 已说明，这里不再重复一行 -->
      <p class="install-note beets-danger">
        Beets 与播放器的协议不一致：{ui.beets.errorMessage ?? 'schema 指纹比对失败'}。
        曲库与配置不受影响，用下面任一方式重装即可。
      </p>
    {/if}

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>预配置环境</strong>
        <span class="beets-dim">
          下载约 34MB 便携版配置包，提供了 
          {#if bundled.length > 0}{bundled.map((p) => p.title).join(' / ')}{:else}基础功能{/if} 
          插件，如果需要使用更多 beets 功能，系统需要 Python 环境支持。
        </span>
      </div>
      <div class="beets-gate">
        <Button
          variant="primary"
          size="sm"
          loading={install.running && install.track === 'bundle'}
          disabled={install.running}
          onclick={() => void ui.installBeets('bundle')}
          title="下载并安装"
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          下载并安装
        </Button>
      </div>
      {@render trackStatus('bundle')}
    </section>

    <!-- 本机安装：只有这条路能改插件清单；没探测到 Python 就不渲染这张卡 -->
    {#if python?.found === true}
      <section class="beets-panel">
        <div class="beets-panel-head">
          <strong>
            本机安装
            <span class="beets-badge ok">当前 Python 版本：{python.version}</span>
          </strong>
          <span class="beets-dim">
            执行安装脚本，使用本机 Python 隔离环境。插件按需勾选，支持 ffmpeg 等更多功能插件，依赖与工具脚本自动下载。
          </span>
        </div>
        {#if !python.supported}
          <p class="beets-danger">
            Python {python.version} 不在支持窗口（3.10–3.14），建议改用预配置环境。
          </p>
        {/if}
        <p class="beets-path mono" title={python.path ?? ''}>
          {python.path}
          <span class="beets-dim">（来自 {python.source}）</span>
        </p>
        {@render pluginPicker('勾选要启用的插件')}
        <div class="beets-gate">
          <Button
            variant="primary"
            size="sm"
            loading={install.running && install.track === 'installer'}
            disabled={install.running}
            onclick={() => void ui.installBeets('installer', chosen)}
            title="按需勾选安装插件"
          >
            {#snippet icon()}<Download size={14} />{/snippet}
            安装（{chosen.length} 个插件）
          </Button>
        </div>
        {@render trackStatus('installer')}
      </section>
    {:else if python !== null}
      <p class="beets-dim install-note">
        未检测到本机 Python（{python.reason ?? '系统里没有 python'}），本机安装不可用。
        <Button variant="link" size="sm" onclick={() => void ui.probeBeetsPython()}>
          重新检测
        </Button>
      </p>
    {/if}
  {:else}
    {#if mismatch}
      <section class="beets-panel">
        <div class="beets-panel-head">
          <strong class="warn-title">
            <TriangleAlert size={15} /> Beets 与播放器的协议契约不一致
          </strong>
          <span class="beets-dim">
            schema 指纹比对失败，查询会读到对不上的形状，因此整页停用。重装整理组件，
            或升级播放器后重试。
          </span>
        </div>
        <div class="beets-gate">
          <Button
            variant="primary"
            size="sm"
            loading={install.running && install.track === 'installer'}
            disabled={install.running}
            onclick={() => void ui.installBeets('installer', chosen)}
          >
            {#snippet icon()}<Download size={14} />{/snippet}
            重装（本机安装）
          </Button>
          <Button
            size="sm"
            loading={install.running && install.track === 'bundle'}
            disabled={install.running}
            onclick={() => void ui.installBeets('bundle')}
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            重装（预配置环境）
          </Button>
        </div>
        {@render progressLine()}
      </section>
    {/if}

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>组件状态</strong>
        <span class="beets-dim">
          这些是工具自报的事实。入口一栏用来回答
          「为什么用的是这个」——同时装了两份时它是唯一能看出差别的线索。
        </span>
      </div>
      <div class="beets-row">
        <span>beets <b>{ui.beets.beetsVersion ?? '未知版本'}</b></span>
        {#if ui.beets.pythonVersion}
          <span>Python <b>{ui.beets.pythonVersion}</b></span>
        {/if}
        {#if ui.beets.protocolVersion !== null && ui.beets.protocolVersion !== undefined}
          <span>协议 <b>v{ui.beets.protocolVersion}</b></span>
        {/if}
        {#if ui.beets.pythonSupported === false}
          <span class="beets-badge warn">Python 版本不在支持窗口</span>
        {/if}
        {#if ui.beets.libraryLock === 'held'}
          <span class="beets-badge warn" title="另一个进程在写库，查询可能读到旧数据">
            库被占用（{ui.beets.libraryLockHolder ?? '另一个进程'}）
          </span>
        {/if}
        <Button size="sm" onclick={() => void ui.refreshBeets()} title="重新探测一次（最坏要等 30 秒）">
          {#snippet icon()}<RefreshCw size={13} />{/snippet}
          重新检测
        </Button>
      </div>

      <table class="facts">
        <tbody>
          <tr>
            <th>入口来源</th>
            <td>{entryText(ui.beets.entrySource)}</td>
          </tr>
          {#if ui.beets.interpreter}
            <tr>
              <th>启动程序</th>
              <td class="mono" title={ui.beets.interpreter}>{ui.beets.interpreter}</td>
            </tr>
          {/if}
          {#if ui.beets.configDir}
            <tr>
              <th>配置目录</th>
              <td class="mono" title={ui.beets.configDir}>{ui.beets.configDir}</td>
            </tr>
          {/if}
          {#if ui.beets.dataHome}
            <tr>
              <th>数据根</th>
              <td class="mono" title={ui.beets.dataHome}>{ui.beets.dataHome}</td>
            </tr>
          {/if}
        </tbody>
      </table>
    </section>

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>重装 / 换插件</strong>
        <span class="beets-dim">
          重装不碰曲库，也不删你的 beets 库；只是把工具本身重新铺一遍。
        </span>
      </div>
      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running && install.track === 'installer'}
          disabled={install.running || !(python?.found ?? false)}
          onclick={() => void ui.installBeets('installer', chosen)}
        >
          {#snippet icon()}<Download size={14} />{/snippet}
          本机安装{python?.found ? `（Python ${python.version}）` : '（未找到 Python）'}
        </Button>
        <Button
          size="sm"
          loading={install.running && install.track === 'bundle'}
          disabled={install.running}
          onclick={() => void ui.installBeets('bundle')}
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          预配置环境
        </Button>
      </div>
      {@render pluginPicker('换装时可改勾选：勾上的会装依赖，并写进配置的启用清单。')}
      {@render progressLine()}
    </section>

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>卸载组件</strong>
        <span class="beets-dim">
          只删安装目录（预配置环境 / 虚拟环境 / 下载缓存）。你的 beets 曲库与分析库<b>不动</b>——
          它们是你的数据，要清理请用各自的入口。
        </span>
      </div>
      {#if confirming}
        <div class="beets-gate">
          <Button
            variant="danger"
            size="sm"
            loading={uninstall.running}
            disabled={uninstall.running}
            onclick={() => {
              confirming = false;
              void ui.uninstallBeets();
            }}
          >
            {#snippet icon()}<Trash2 size={14} />{/snippet}
            确认卸载
          </Button>
          <Button size="sm" disabled={uninstall.running} onclick={() => (confirming = false)}>取消</Button>
        </div>
      {:else}
        <div class="beets-gate">
          <Button size="sm" disabled={uninstall.running} onclick={() => (confirming = true)}>
            {#snippet icon()}<Trash2 size={14} />{/snippet}
            卸载组件
          </Button>
        </div>
      {/if}
      {@render uninstallLine()}
    </section>
  {/if}
</div>

<style>
  /* 状态行：裸文本不进卡，与两张安装卡同列间距 */
  .install-note {
    margin: 0;
  }

  /* 卡内安装状态：跟在按钮正下方；报错换警示色（scoped 类避免被基类压掉） */
  .track-status {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-3);
  }

  .track-status-error {
    color: var(--danger);
  }

  .beets-path {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
    color: var(--text-3);
    word-break: break-all;
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
    gap: 6px;
    flex-wrap: wrap;
    font-size: var(--fs-base);
    cursor: pointer;
  }

  .pick input {
    accent-color: var(--accent);
  }

  .pick-name {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
  }

  .pick-title {
    color: var(--text-2);
  }

  .pick-tag {
    padding: 0 5px;
    border-radius: 4px;
    border: 1px solid var(--line-1);
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
    font-size: var(--fs-base);
  }

  .facts th {
    text-align: left;
    font-weight: 400;
    color: var(--text-3);
    width: 82px;
    padding: 3px 0;
    vertical-align: top;
  }

  .facts td {
    padding: 3px 0;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 0; /* 配合 table-layout 让省略号生效 */
  }

  .facts .mono {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .warn-title {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--warn);
  }
</style>
