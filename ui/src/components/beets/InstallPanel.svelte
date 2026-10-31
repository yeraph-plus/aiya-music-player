<script lang="ts">
  import { onMount } from 'svelte';
  import { Download, Package, RefreshCw, Trash2, TriangleAlert } from '@lucide/svelte';
  import Button from '../ui/Button.svelte';
  import { ui } from '../../stores/ui.svelte';

  /**
   * 安装子页：Beets 装没装、装的是哪一版、能不能干活（单轨，docs/58）。
   *
   * 唯一的分发物是**预配置环境**（便携包）：自带解释器、pip 与预置插件
   * （内置可用的四件 + 分析器全家桶），下载 → 校验 → 解压全自动。
   *
   * 未安装 = 一张卡一个按钮；已安装 = 一张状态卡三个动作：
   * - **更新**：重新下载并解压（Rust 直连）；
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

  onMount(() => {
    // 内嵌插件表在二进制里，不依赖"已装组件"
    void ui.loadBeetsPlugins();
  });

  function toggle(name: string): void {
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    picked = [...next];
  }

  /** 安装阶段徽章文本：宿主发的 stage 词 → 人话；未知词原样显示 */
  const stageText: Record<string, string> = {
    prepare: '准备',
    download: '下载',
    install: '安装',
    unpack: '解压',
    bin: '外置工具',
    pip: 'pip',
  };
  const stageLabel = (s: string): string => stageText[s] ?? s;

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

<!-- 卡内安装状态：跟在按钮正下方——阶段是徽章，消息是文本 -->
{#snippet status()}
  {#if install.running}
    <p class="track-status">
      <span class="beets-badge">{stageLabel(install.stage)}</span>
      <span>{install.message}</span>
    </p>
  {:else if install.error}
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
        曲库与配置不受影响，重新下载安装即可。
      </p>
    {/if}

    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>预配置环境</strong>
        <span class="beets-dim">
          下载约 75MB 便携包，自带解释器、pip 与
          {#if bundled.length > 0}{bundled.map((p) => p.title).join(' / ')}{:else}基础功能{/if}
          插件（含分析器与模型），下载、校验、解压自动完成，不需要本机 Python。
        </span>
      </div>
      <div class="beets-gate">
        <Button
          variant="primary"
          size="sm"
          loading={install.running}
          disabled={install.running}
          onclick={() => void ui.installBeets()}
          title="下载并安装"
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          下载并安装
        </Button>
      </div>
      {@render status()}
    </section>
  {:else}
    {#if mismatch}
      <section class="beets-panel">
        <div class="beets-panel-head">
          <strong class="warn-title">
            <TriangleAlert size={15} /> Beets 与播放器的协议契约不一致
          </strong>
          <span class="beets-dim">
            schema 指纹比对失败，查询会读到对不上的形状，因此整页停用。重新下载安装，
            或升级播放器后重试。
          </span>
        </div>
        <div class="beets-gate">
          <Button
            variant="primary"
            size="sm"
            loading={install.running}
            disabled={install.running}
            onclick={() => void ui.installBeets()}
          >
            {#snippet icon()}<Package size={14} />{/snippet}
            重新下载安装
          </Button>
        </div>
        {@render status()}
      </section>
    {/if}

    <!-- 装后状态卡：一张卡，更新 / 换插件 / 删除三个动作 -->
    <section class="beets-panel">
      <div class="beets-panel-head">
        <strong>预配置环境</strong>
        <span class="beets-dim">
          自带解释器与 pip。更新 = 重新下载；换插件 = 包内重跑安装脚本
          （pip 装/卸与外部工具自动处理）；删除 = 组件目录整体移除。
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

      {@render pluginPicker('换插件时改勾选：确认后包内重跑安装脚本，pip 装/卸自动完成。')}
      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running}
          disabled={install.running}
          onclick={() => void ui.setBeetsPlugins(chosen)}
          title="在包内环境按勾选变更插件"
        >
          {#snippet icon()}<Download size={14} />{/snippet}
          换插件（{chosen.length} 个插件）
        </Button>
      </div>
      {@render status()}

      <div class="beets-gate">
        <Button
          size="sm"
          loading={install.running}
          disabled={install.running}
          onclick={() => void ui.installBeets()}
          title="重新下载并解压，更新到通道最新"
        >
          {#snippet icon()}<Package size={14} />{/snippet}
          重新下载并安装（更新到通道最新）
        </Button>
      </div>

      <div class="beets-gate">
        {#if confirming}
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
            确认删除
          </Button>
          <Button size="sm" disabled={uninstall.running} onclick={() => (confirming = false)}>取消</Button>
        {:else}
          <Button size="sm" disabled={uninstall.running} onclick={() => (confirming = true)}>
            {#snippet icon()}<Trash2 size={14} />{/snippet}
            删除组件
          </Button>
        {/if}
      </div>
      <p class="install-note">
        删除只动组件目录（预配置环境与下载缓存），
        你的 beets 曲库与分析库<b>不动</b>——它们是你的数据，要清理请用各自的入口。
      </p>
      {@render uninstallLine()}
    </section>
  {/if}
</div>

<style>
  /* 状态行：裸文本不进卡，与安装卡同列间距 */
  .install-note {
    margin: 0;
  }

  /* 卡内安装状态：阶段徽章 + 消息文本，跟在按钮正下方；报错换警示色 */
  .track-status {
    display: flex;
    align-items: center;
    gap: 6px;
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
