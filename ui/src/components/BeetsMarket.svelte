<script lang="ts">
  import { Puzzle } from '@lucide/svelte';
  import Button from './ui/Button.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 插件市场（docs/40 §3 拍板：白名单版）。
   *
   * 设计要点（都是拍过板的，别在实现里悄悄改）：
   * 1. **白名单**：目录来自工具侧 `market_report()`，只收录本地实测过的
   *    插件——UI 不做"全量市场"，也不提供任意 pip 包名输入框。
   * 2. **目录懒加载**：`<details>` 收起时不拉数据。目录是一次性短进程
   *    查询（~1s），展开即拉、装完由 pluginDone 的复检报告直接更新，
   *    不做轮询也不做缓存失效——插件装没装只有这两条路会变。
   * 3. **三态徽标**：ready（可用）/ missing_deps（缺依赖）/ unavailable
   *    （未安装）。「安装」按钮只对 `pypi:<包名>` 档且非 ready 的项出现；
   *    builtin 档随 beets 内置，装与不装没有意义（启停归 config，本区
   *    不做开关——那是插件启用配置的事，不属于"市场"）。
   * 4. **会话槽共享**：安装走 `beets:maintenance`，与维护四件套互斥。
   *    任何 beets 会话在跑，安装按钮都禁用（不是只禁安装中的那个）。
   */
  const market = $derived(ui.beetsMarket);
  const loadError = $derived(ui.beetsMarketError);
  const maint = $derived(ui.beetsMaint);
  const busy = $derived(maint.running);
  const installing = $derived(maint.running && maint.kind === 'plugin');

  const stateLabel: Record<string, string> = {
    ready: '可用',
    missing_deps: '缺依赖',
    unavailable: '未安装',
  };

  /** pypi 档显示包名（安装按钮的 title 也用它） */
  function pypiPackage(source: string): string {
    return source.startsWith('pypi:') ? source.slice(5) : '';
  }

  function onToggle(e: Event): void {
    const open = (e.currentTarget as HTMLDetailsElement | null)?.open === true;
    if (open && ui.beetsMarket === null && ui.beetsMarketError === null) {
      void ui.loadBeetsMarket();
    }
  }

  function install(name: string): void {
    // 失败时 store 内部落 ui.beetsMaint.error，由下方 danger 行显示
    void ui.installBeetsPlugin(name);
  }
</script>

<details class="maint-fold market-fold" ontoggle={onToggle}>
  <summary>
    <span><Puzzle size={14} /> 插件市场</span>
    <span class="dim">白名单插件 · 装进整理库的运行环境（pip / uv 自动探测）</span>
  </summary>

  <div class="market-body">
    {#if loadError}
      <div class="market-error">
        <span class="danger">{loadError}</span>
        <Button size="sm" onclick={() => void ui.loadBeetsMarket()}>重试</Button>
      </div>
    {:else if market === null}
      <span class="dim">正在读取插件目录…</span>
    {:else}
      <ul class="market-list">
        {#each market as item (item.name)}
          {@const isPypi = item.source.startsWith('pypi:')}
          <li>
            <div class="market-head">
              <b>{item.name}</b>
              <span class="badge badge-{item.state}">{stateLabel[item.state] ?? item.state}</span>
              <span class="chip">{isPypi ? `pip 装到当前环境` : 'beets 内置'}</span>
            </div>
            <div class="market-note">
              {item.note}
              {#if item.missing.length > 0}
                <span class="danger">缺 {item.missing.join('、')}</span>
              {/if}
            </div>
            {#if isPypi && item.state !== 'ready'}
              <div class="market-action">
                <Button
                  size="sm"
                  disabled={busy}
                  title="pip 安装 {pypiPackage(item.source)} 到 beets 所在的 Python 环境"
                  onclick={() => install(item.name)}
                >
                  安装
                </Button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>

      {#if installing}
        <div class="market-progress">
          正在安装…{maint.stage ? ` ${maint.stage}` : ''}
        </div>
      {:else if maint.kind === 'plugin' && maint.summary}
        <div class="market-progress"><b>{maint.summary}</b></div>
      {/if}
      {#if maint.kind === 'plugin' && maint.error}
        <div class="danger">{maint.error}</div>
      {/if}
    {/if}
  </div>
</details>

<style>
  /* 组件内自带样式：dim/danger/chip 在全局只以后代选择器形式存在，
     直接用在本组件不会命中——复制一份比去改全局更安全（同 BeetsMaintenance）。 */
  .market-fold summary {
    display: flex;
    align-items: baseline;
    gap: 8px;
    cursor: pointer;
  }

  .market-fold summary span:first-child {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .market-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px 0 4px;
  }

  .market-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .market-list > li {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--line);
  }

  .market-list > li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .market-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .market-note {
    font-size: 12px;
    color: var(--text-2);
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .market-action {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .market-progress {
    font-size: 12px;
    color: var(--text-2);
    word-break: break-all;
  }

  .market-error {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dim {
    color: var(--text-3);
    font-size: 12px;
  }

  .danger {
    color: var(--danger, #e5484d);
    font-size: 12px;
  }

  .chip {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 6px;
    background: var(--surface-3);
    color: var(--text-2);
    font-size: 11px;
  }

  .badge {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 6px;
    font-size: 11px;
  }

  .badge-ready {
    background: var(--surface-3);
    color: var(--success, #46a758);
  }

  .badge-missing_deps,
  .badge-unavailable {
    background: var(--surface-3);
    color: var(--text-3);
  }
</style>
