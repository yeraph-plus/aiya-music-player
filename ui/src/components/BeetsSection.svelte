<script lang="ts">
  import { Download, Package } from '@lucide/svelte';
  import BeetsMaintenance from './BeetsMaintenance.svelte';
  import ImportPanel from './ImportPanel.svelte';
  import Button from './ui/Button.svelte';
  import type { BeetsInstallTrack } from '../lib/ipc';
  import { shortenPath } from '../lib/format';
  import { list } from '../stores/list.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 整理库段（扫描入库页第二组）。
   *
   * 这里管的是**另一个库**：beets 的 `library.db` 与它配置的目录。和第一组
   * 的曲库（`tracks` 表）是两套数据，两组之间只有"read model 对齐"一条通路。
   *
   * 为什么整段在 beets 未装时收成一行：docs/33 的原则是"beets 是增强能力，
   * 不是播放依赖"。以前导入面板与维护四件套常驻，没装的人看到的是十几个
   * 灰色按钮——那不是"待安装"，那是噪音。
   *
   * 命名（本轮统一）：播放器这边叫「曲库」，beets 这边叫「整理库」。
   * 两个都叫"库"是上一轮术语打架的根源。
   */
  const beets = $derived(ui.beets);
  const install = $derived(ui.beetsInstall);
  /** 装好且协议对得上才展开导入/维护——否则下面全是灰按钮 */
  const usable = $derived(beets?.found === true);

  /**
   * 工具入口是从哪找到的（docs/33 §6.1 四种来源）。
   *
   * 排障的第一个问题永远是"为什么用的是这个"，所以让发现层直接回答，
   * 而不是让用户去翻日志。收在一行里，不单独占一块。
   */
  function entrySourceText(source: string | null): string {
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

  /** 安装轨道的中文说法（两条轨道对用户的意义不同：要不要系统 Python） */
  function installTrackText(track: BeetsInstallTrack): string {
    return track === 'bundle' ? '免安装包' : '快速安装';
  }
</script>

<div class="beets-section">
  <div class="beets-tool-block">
    {#if beets === null}
      <span class="dim">正在检测整理组件…</span>
    {:else if usable}
      {@const b = beets}
      <div class="beets-tool-head">
        <strong>整理库</strong>
        <span class="dim">
          beets {b.beetsVersion ?? '未知版本'} · Python {b.pythonVersion ?? '未知'}
        </span>
        {#if b.pythonSupported === false}
          <span class="danger">运行时不在支持窗口内</span>
        {/if}
      </div>
      <div class="beets-tool-meta">
        <span>库根
          <b class="path" title={b.libraryDirectory ?? ''}>
            {b.libraryDirectory ? shortenPath(b.libraryDirectory) : '未配置（不会重排文件）'}
          </b>
        </span>
        <span>入口 <b>{entrySourceText(b.entrySource)}</b></span>
      </div>
      {#if b.pluginSummary}
        {@const bad = (b.pluginHealth ?? []).filter((p) => p.missing.length > 0)}
        <div class="beets-plugins">
          <span class="dim">
            插件 {b.pluginSummary.ready} 可用{#if b.pluginSummary.missingDeps} · {b.pluginSummary.missingDeps} 缺依赖{/if}{#if b.pluginSummary.unavailable} · {b.pluginSummary.unavailable} 不可用{/if}
          </span>
          {#if bad.length > 0}
            <ul>
              {#each bad as p (p.name)}
                <li>
                  <b>{p.name}</b> 缺 {p.missing.join('、')}
                  {#if p.note}<span class="dim"> · {p.note}</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    {:else if install.running}
      <div class="beets-tool-head">
        <strong>正在安装整理组件</strong>
        <span class="dim">
          {installTrackText(install.track)} · {install.stage}
          {install.message ? `· ${install.message}` : ''}
        </span>
      </div>
    {:else}
      <div class="beets-tool-head">
        {#if beets?.errorCode === 'E_PROTOCOL_MISMATCH'}
          <!-- 组件在，但说的不是同一版协议。装一份新的就是"升级"，所以下面
               两个按钮照旧可用——文案必须说清这不是"没装"。 -->
          <strong>整理组件版本不匹配</strong>
          <span class="dim">{beets.errorMessage ?? '重装或升级即可，曲库与配置不受影响'}</span>
        {:else}
          <strong>整理库未启用</strong>
          <span class="dim">不影响播放。装上之后才能做标签匹配、批量改标签与文件重排。</span>
        {/if}
      </div>
      <div class="beets-tool-actions">
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
      {#if install.error}
        <span class="danger">{install.error}</span>
      {/if}
    {/if}
  </div>

  {#if usable}
    <ImportPanel onsynced={() => void list.reload({ restore: true })} />

    <details class="maint-fold">
      <summary>
        <span>整理已有曲目</span>
        <span class="dim">
          登记进整理库 · 按整理库重写标签 · 查找重复 · 按整理库重排文件
        </span>
      </summary>
      <BeetsMaintenance />
    </details>
  {/if}
</div>
