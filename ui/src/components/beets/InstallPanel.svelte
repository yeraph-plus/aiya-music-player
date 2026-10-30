<script lang="ts">
  import { Download, Package, RefreshCw, TriangleAlert } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import { ui } from '../../stores/ui.svelte';
  import type { BeetsInstallTrack } from '../../lib/ipc';

  /**
   * 安装子页：整理组件装没装、装的是哪一版、能不能干活。
   *
   * 为什么单独一页而不是在别的页顶上一块提示：这一页回答的是**与曲库无关**的
   * 前置问题（组件、Python、协议指纹、插件依赖）。以前它和查询面板挤在同一屏，
   * 结果是"没装"的时候整页都是灰按钮，用户根本看不出哪一步出了问题。
   *
   * 两条安装轨道的差别是**要不要本机 Python**（docs/33 §6.2）：
   * - 快速安装：下载 ~33KB 安装器，用本机 Python 建 venv；
   * - 免安装包：下载 ~31MB 套件，自带解释器，解压即用。
   * 这段说明留在按钮旁边而不折叠：选错轨道的人会在几分钟后卡在
   * "找不到 Python"上，那时他已经不记得自己点的是哪个按钮。
   */

  const found = $derived(ui.beets?.found === true);
  const mismatch = $derived(found && ui.beets?.schemaMatched === false);
  const install = $derived(ui.beetsInstall);

  function trackText(track: BeetsInstallTrack): string {
    return track === 'bundle' ? '免安装包' : '快速安装';
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
      <div class="beets-gate">
        <Button
          variant="primary"
          size="sm"
          loading={install.running && install.track === 'installer'}
          disabled={install.running}
          onclick={() => void ui.installBeets('installer')}
          title="下载安装器（约 33KB），用系统 Python 建环境"
        >
          {#snippet icon()}<Download size={14} />{/snippet}
          快速安装
        </Button>
        <Button
          size="sm"
          loading={install.running && install.track === 'bundle'}
          disabled={install.running}
          onclick={() => void ui.installBeets('bundle')}
          title="下载自带运行时的套件包（约 31MB），不需要系统 Python"
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          免安装包
        </Button>
        <span class="beets-dim">
          快速安装要求机器上有 Python 3.10–3.14；没有就用免安装包（自带解释器，解压即用）。
        </span>
      </div>
      {#if install.running}
        <p class="beets-dim">
          {trackText(install.track)} · {install.stage}
          {install.message ? `· ${install.message}` : ''}
        </p>
      {:else if install.error}
        <p class="beets-danger">{install.error}</p>
      {/if}
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
            loading={install.running}
            disabled={install.running}
            onclick={() => void ui.installBeets('installer')}
          >
            {#snippet icon()}<Download size={14} />{/snippet}
            重装（快速安装）
          </Button>
          <Button
            size="sm"
            disabled={install.running}
            onclick={() => void ui.installBeets('bundle')}
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            重装（免安装包）
          </Button>
        </div>
        {#if install.running}
          <p class="beets-dim">{trackText(install.track)} · {install.stage} · {install.message}</p>
        {:else if install.error}
          <p class="beets-danger">{install.error}</p>
        {/if}
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
          但它给不出字段。这里报的是**实测三态**。
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
    </section>

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>重装 / 换轨道</strong>
        <span class="beets-dim">
          重装不碰曲库，也不删你的 beets 库；只是把工具本身重新铺一遍。
        </span>
      </div>
      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running && install.track === 'installer'}
          disabled={install.running}
          onclick={() => void ui.installBeets('installer')}
        >
          {#snippet icon()}<Download size={14} />{/snippet}
          快速安装
        </Button>
        <Button
          size="sm"
          loading={install.running && install.track === 'bundle'}
          disabled={install.running}
          onclick={() => void ui.installBeets('bundle')}
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          免安装包
        </Button>
      </div>
      {#if install.running}
        <p class="beets-dim">{trackText(install.track)} · {install.stage} · {install.message}</p>
      {:else if install.error}
        <p class="beets-danger">{install.error}</p>
      {/if}
    </section>
  {/if}
</div>

<style>
  /* 状态表：左列窄、右列吃掉剩余宽度并单行省略。
     用 table 而不是 grid 是因为行数随字段有无变化（有的工具不报 pythonVersion） */
  .facts {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
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
    font-size: 11.5px;
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
    font-size: 12px;
  }

  /* 未启用的插件压暗但保留——它解释了"为什么某个字段查不到" */
  .plugin.disabled {
    opacity: 0.6;
  }

  .plugin-name {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: 11.5px;
  }
</style>
