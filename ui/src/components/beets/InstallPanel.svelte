<script lang="ts">
  import { onMount } from 'svelte';
  import { Download, Package, RefreshCw, Trash2, TriangleAlert } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import { ui } from '../../stores/ui.svelte';
  import type { BeetsInstallTrack } from '../../lib/ipc';

  /**
   * 安装子页：整理组件装没装、装的是哪一版、能不能干活。
   *
   * 这一页回答的是**与曲库无关**的前置问题（组件、Python、协议指纹、插件依赖）。
   * 以前它和查询面板挤在同一屏，结果是"没装"的时候整页都是灰按钮，用户看不出
   * 哪一步出了问题。
   *
   * ## 两条路径（docs/51 §4）
   *
   * 差别只有一个问题：**机器上有没有 Python**。
   * - 免配置包：下载约 34MB，自带解释器 + 预置插件，不需要任何前置；
   * - 本机 Python：只下约 100KB 安装器，在本机建 venv，插件由用户勾。
   *
   * 所以这一页把两条路并排摆在"未安装"状态下，各自带自己的前置条件与代价。
   * 只说"选一个"是不够的——选错的人会在几分钟后卡在"找不到 Python"上，
   * 那时他已经不记得自己点的是哪个按钮。
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
    return track === 'bundle' ? '免配置包' : '本机 Python';
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
      case 'dev':
        return '源码树（开发模式）';
      default:
        return source ?? '未知';
    }
  }

  const healthLabel: Record<string, string> = {
    ready: '可用',
    missing_deps: '缺依赖',
    unavailable: '不可用',
  };
</script>

{#snippet progressLine()}
  {#if install.running}
    <p class="beets-dim">{trackText(install.track)} · {install.stage} · {install.message}</p>
  {:else if install.error}
    <p class="beets-danger">{install.error}</p>
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
            {#if p.exe.length > 0}
              <!-- 不再是"自备"：安装时按 docs/53 自动下载（ffmpeg 约 30MB 下载） -->
              <span class="pick-tag warn">另下 {p.exe.join(' / ')}</span>
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
      <p class="beets-dim">正在检测整理组件…</p>
    </section>
  {:else if !found}
    <section class="beets-panel">
      <div class="beets-panel-head">
        {#if ui.beets.errorCode === 'E_PROTOCOL_MISMATCH'}
          <strong>整理组件版本不匹配</strong>
          <span class="beets-dim">
            {ui.beets.errorMessage ?? '重装或升级即可，曲库与配置不受影响'}
          </span>
        {:else}
          <strong>整理库未启用</strong>
          <span class="beets-dim">
            不影响播放。装上之后才能索引曲库、按标签/评分/插件字段（如 bpm、情绪）
            查询，以及把新文件按「艺人/专辑」整理进曲库。
          </span>
        {/if}
      </div>

      <!-- 两条路径并排：差别只有"有没有 Python" -->
      <div class="tracks">
        <article class="track">
          <h4>
            免配置包
            <span class="beets-badge accent">不需要 Python</span>
          </h4>
          <p class="beets-dim">
            下载约 34MB，自带解释器，并已预置
            {#if bundled.length > 0}{bundled.map((p) => p.title).join(' / ')}{:else}常用插件{/if}。
            解压即用，换台机器也不用重装 Python。
          </p>
          <Button
            variant="primary"
            size="sm"
            loading={install.running && install.track === 'bundle'}
            disabled={install.running}
            onclick={() => void ui.installBeets('bundle')}
            title="下载自带运行时的免配置包，不需要系统 Python"
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            下载并安装
          </Button>
        </article>

        <article class="track">
          <h4>
            本机 Python
            <span class="beets-badge" class:warn={python?.found && !python.supported}>
              {#if python === null}检测中…{:else if python.found}已找到 {python.version}{:else}未找到{/if}
            </span>
          </h4>
          <p class="beets-dim">
            只下载约 100KB 安装器，用本机的 Python 建一个隔离环境（不污染全局），
            插件由你勾。
          </p>

          {#if python === null}
            <p class="beets-dim">正在检测本机 Python…</p>
          {:else if !python.found}
            <p class="beets-dim">{python.reason ?? '没有找到可用的 Python。'}</p>
            <Button size="sm" onclick={() => void ui.probeBeetsPython()} title="再检测一次">
              {#snippet icon()}<RefreshCw size={13} />{/snippet}
              重新检测
            </Button>
          {:else}
            {#if !python.supported}
              <p class="beets-danger">
                Python {python.version} 不在支持窗口（3.10–3.14），装了也可能跑不起来。
                建议改用左边的免配置包。
              </p>
            {/if}
            <p class="beets-path mono" title={python.path ?? ''}>
              {python.path}
              <span class="beets-dim">（来自 {python.source}）</span>
            </p>
            <Button
              variant="primary"
              size="sm"
              loading={install.running && install.track === 'installer'}
              disabled={install.running}
              onclick={() => void ui.installBeets('installer', chosen)}
              title="用本机 Python 建环境并安装勾选的插件"
            >
              {#snippet icon()}<Download size={14} />{/snippet}
              安装（{chosen.length} 个插件）
            </Button>
          {/if}
        </article>
      </div>

      {@render pluginPicker('勾选要启用的插件（装了不启用等于白装）')}
      {@render progressLine()}
    </section>
  {:else}
    {#if mismatch}
      <section class="beets-panel">
        <div class="beets-panel-head">
          <strong class="warn-title">
            <TriangleAlert size={15} /> 整理组件与播放器的协议契约不一致
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
            重装（本机 Python）
          </Button>
          <Button
            size="sm"
            loading={install.running && install.track === 'bundle'}
            disabled={install.running}
            onclick={() => void ui.installBeets('bundle')}
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            重装（免配置包）
          </Button>
        </div>
        {@render progressLine()}
      </section>
    {/if}

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>组件状态</strong>
        <span class="beets-dim">
          这些是工具自报的事实（docs/33 M2 之后宿主不再自己探测）。入口一栏用来回答
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
        <strong>插件</strong>
        <span class="beets-dim">
          「配置里写了」不等于「能干活」——缺 Python 依赖的插件依然会出现在插件列表里，
          但它给不出字段。这里报的是实测三态。
        </span>
      </div>
      {#if ui.beets.pluginHealth && ui.beets.pluginHealth.length > 0}
        <ul class="plugins">
          {#each ui.beets.pluginHealth as p (p.name)}
            <li class="plugin" class:disabled={!p.enabled}>
              <span class="plugin-name">{p.name}</span>
              <span class="beets-badge" class:accent={p.state === 'ready'} class:warn={p.state === 'missing_deps'}>
                {healthLabel[p.state] ?? p.state}
              </span>
              {#if p.missing.length > 0}
                <span class="beets-dim">缺 {p.missing.join(' / ')}</span>
              {/if}
              {#if !p.enabled}
                <span class="beets-dim">（配置里未启用）</span>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="beets-dim">没有插件自检结果（老版本工具不报这一项）。</p>
      {/if}
      {#if ui.beets.pluginSummary}
        <p class="beets-dim">
          {ui.beets.pluginSummary.ready} 个可用{#if ui.beets.pluginSummary.missingDeps} · {ui.beets
              .pluginSummary.missingDeps} 个缺依赖{/if}
        </p>
      {/if}
      {@render pluginPicker('换装时可改勾选：勾上的会装依赖，并写进配置的启用清单。')}
    </section>

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>重装 / 换路径</strong>
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
          本机 Python{python?.found ? `（${python.version}）` : '（未找到）'}
        </Button>
        <Button
          size="sm"
          loading={install.running && install.track === 'bundle'}
          disabled={install.running}
          onclick={() => void ui.installBeets('bundle')}
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          免配置包
        </Button>
      </div>
      {@render progressLine()}
    </section>

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>卸载组件</strong>
        <span class="beets-dim">
          只删安装目录（虚拟环境 / 套件包 / 下载缓存）。整理库与分析库<b>不动</b>——
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
  /* 两条路径并排：窄屏时自动堆叠（不写死列数，靠 minmax 断开） */
  .tracks {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 10px;
  }

  .track {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    border: 1px solid var(--line-1);
    border-radius: 8px;
    background: var(--surface-2);
  }

  .track h4 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: var(--fs-base);
    font-weight: 600;
  }

  .track p {
    margin: 0;
    font-size: var(--fs-base);
    line-height: 1.55;
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

  .pick-tag.warn {
    color: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 40%, transparent);
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

  .plugins {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .plugin {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: var(--fs-base);
  }

  /* 未启用的插件压暗但保留——它解释了"为什么某个字段查不到" */
  .plugin.disabled {
    opacity: 0.6;
  }

  .plugin-name {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: var(--fs-sm);
  }
</style>
