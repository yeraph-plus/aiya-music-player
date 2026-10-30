<script lang="ts">
  import { onMount } from 'svelte';
  import { Wrench } from '@lucide/svelte';
  import Header from '../components/content/Header.svelte';
  import Toolbar from '../components/content/Toolbar.svelte';
  import Button from '../components/ui/Button.svelte';
  import EmptyState from '../components/ui/EmptyState.svelte';
  import InstallPanel from '../components/beets/InstallPanel.svelte';
  import IndexPanel from '../components/beets/IndexPanel.svelte';
  import OrganizePanel from '../components/beets/OrganizePanel.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 整理库（beets）页的壳：**安装 / 索引 / 整理 三个子页**。
   *
   * ## 为什么分页（2026-10-30 用户拍板）
   *
   * 三件事的心智模型完全不同，挤在一屏里必然互相污染：
   * - **安装**是前置条件（组件、Python、协议指纹、插件依赖），一次做完就不再管；
   * - **索引**是**读**：把曲库登记成只读镜像，然后查询、挑歌、存歌单；
   * - **整理**是**写**：把待整理目录里的新文件搬进曲库（改标签、挪文件）。
   *
   * 以前的实现把三者写在一屏，症状是"没装"的时候整页灰、分不清哪一步坏了；
   * 装上之后写动作（整理）又躲在读动作（查询）下面，误点的代价是文件被移走。
   *
   * ## 没装 / 协议不符的门放在壳里
   *
   * 索引页与整理页都需要"组件可用"这个前提。各自写一遍门 = 两份措辞不同的
   * 同一句话；放在壳里只写一次，且子页可以假设前提成立。
   *
   * 门不是"禁用 tab"：tab 永远可点（用户要能看见那一页在讲什么），点进去看到
   * 的是一句人话 + 一个「去安装」按钮。
   */

  const TABS = [
    { id: 'install', label: '安装' },
    { id: 'index', label: '索引' },
    { id: 'organize', label: '整理' },
  ];

  const found = $derived(ui.beets?.found === true);
  /** 协议指纹对不上 = 装的组件与播放器不是同一版 schema，读出来的形状无意义 */
  const schemaMismatch = $derived(found && ui.beets?.schemaMatched === false);
  const ready = $derived(found && !schemaMismatch);

  const headerMeta = $derived.by(() => {
    if (ui.beets === null) return '正在检测整理组件…';
    if (!found) return '未启用（不影响播放）';
    if (schemaMismatch) return '组件版本不匹配';
    return [`beets ${ui.beets.beetsVersion ?? '未知版本'}`, '曲库只读分析 + 新文件整理'].join(' · ');
  });

  onMount(() => {
    void (async () => {
      // 进页面就重探一次，而不是"只有 null 才探"：`ui.beets` 是**启动时**那个
      // 快照，启动那几秒里组件可能还没装好、或上一个会话正占着库。按 found=false
      // 认命的话，用户装完组件还得重启应用才能用——实测踩过。
      if (!ui.beets || ui.beets.found !== true) await ui.refreshBeets();
      // 探测到没装就翻到安装页——但只在用户没手动选过的时候。手动点过「索引」
      // 两秒后又被拽回去，比一开始就停在错误的页更让人恼火。
      if (ui.beets?.found !== true && !ui.beetsTabTouched) ui.beetsTab = 'install';
    })();
  });

  function goInstall(): void {
    ui.beetsTabTouched = true;
    ui.beetsTab = 'install';
  }
</script>

<div class="page-scroll beets-page">
  <Header title="整理库" big meta={headerMeta} />

  <Toolbar
    tabs={TABS}
    active={ui.beetsTab}
    ontab={(id) => {
      ui.beetsTabTouched = true;
      ui.beetsTab = id as 'install' | 'index' | 'organize';
    }}
  />

  {#if ui.beetsTab === 'install'}
    <InstallPanel />
  {:else if !ready}
    <section class="gate-panel">
      <EmptyState
        title={schemaMismatch ? '组件与播放器版本不一致' : '整理组件未启用'}
        hint={schemaMismatch
          ? 'schema 指纹比对失败。索引与整理都依赖同一份协议，装错了版本读出来的形状会对不上，所以先停在这里。'
          : '这一页要做的事都要 beets 组件在场。装上之后曲库与配置都不受影响，播放也照常。'}
        fill
      >
        {#snippet icon()}<Wrench size={26} />{/snippet}
        {#snippet action()}
          <Button variant="primary" size="sm" onclick={goInstall}>去安装</Button>
        {/snippet}
      </EmptyState>
    </section>
  {:else if ui.beetsTab === 'index'}
    <IndexPanel />
  {:else}
    <OrganizePanel />
  {/if}
</div>

<style>
  .gate-panel {
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-2);
    padding: 24px 16px;
  }
</style>
