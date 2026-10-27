<script lang="ts">
  import { ChevronRight, RefreshCw } from '@lucide/svelte';
  import BeetsSection from '../components/BeetsSection.svelte';
  import LibraryTools from '../components/LibraryTools.svelte';
  import Button from '../components/ui/Button.svelte';
  import Header from '../components/content/Header.svelte';
  import { formatDateTime, shortenPath } from '../lib/format';
  import { history } from '../stores/history.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 扫描入库页（docs/29 批 A 从设置页拆出；本轮按数据对象重排）。
   *
   * 排版原则（本轮改动的全部理由）：**按对象分组，不按组件分组**。
   *
   * 这一页此前是七个并列段落，分别属于三条互不相干的功能线（播放器曲库、
   * beets 整理库、诊断），术语也各说各的——最典型的是「扫描目录」与
   * 「导入目录」看起来是同一件事的两个按钮，实际一个写 `tracks` 表、
   * 一个写 beets 的 `library.db`；两个都叫"库"，但只有前者是播放依赖。
   *
   * 现在只剩两组：**曲库**（必有）与**整理库**（可选，未装时收成一行）。
   * 诊断只留下"出问题时唯一有用"的三样（版本/数据目录/最近错误），
   * 收进折叠区——它们是给排障用的，不该占主路由的视觉重量。
   *
   * 下放出去的东西（不要去这里找）：
   * - 快捷键冲突 → 设置 → 快捷键（那边本来就有，这里是重复的第二份）
   * - 媒体控制 / 防休眠 / 托盘 → 设置 → 系统（各自开关旁边一行状态）
   * - 自动刷新 → 曲库段（它就是那条管线）
   * - 媒体流 / 最近查询 → 已删（前者是协议层泄漏自检，后者是 SQL 性能排查，
   *   两者都属于开发者视角；真要排查看日志）
   */
  const diagnostics = $derived(ui.diagnostics);
  const recentErrors = $derived(diagnostics?.recentErrors ?? []);
</script>

<div class="page-scroll tools-page">
  <Header kicker="LIBRARY TOOLS" title="扫描入库" />

  <section class="tools-section" id="tools-library">
    <div class="section-heading">
      <div>
        <h2>曲库</h2>
        <p>播放器自己的索引：读文件里的标签与时长，记住位置。只读文件，不改标签。</p>
      </div>
    </div>
    <LibraryTools />
  </section>

  <section class="tools-section" id="tools-beets">
    <div class="section-heading">
      <div>
        <h2>整理库</h2>
        <p>
          可选。beets 维护的另一套库，负责联网匹配标签、批量改写与按模板重排文件；
          改完会单向对齐回曲库。
        </p>
      </div>
    </div>
    <BeetsSection />
  </section>

  <section class="tools-section" id="tools-advanced">
    <details class="advanced-fold">
      <summary>
        <ChevronRight size={13} class="fold-arrow" />
        <span>高级与诊断</span>
        <span class="dim">版本、数据目录、最近的错误</span>
      </summary>

      {#if diagnostics}
        <div class="advanced-body">
          <div class="diag-grid">
            <div><span>版本</span><b>{diagnostics.version}</b></div>
            <div><span>平台</span><b>{diagnostics.platform}</b></div>
            <div><span>Schema</span><b>v{diagnostics.schemaVersion}</b></div>
            <div><span>播放列表</span><b>{diagnostics.playlists}</b></div>
            <div class="wide"><span>数据目录</span><b class="path" title={diagnostics.dataDir}>{shortenPath(diagnostics.dataDir)}</b></div>
          </div>

          <div class="diag-actions">
            <Button size="sm" onclick={() => void ui.refreshDiagnostics()}>
              {#snippet icon()}<RefreshCw size={13} />{/snippet}
              重新读取
            </Button>
            <Button size="sm" onclick={() => ui.openDataDir()}>打开数据目录</Button>
            <Button variant="danger" size="sm" onclick={() => void history.clear()}>清空播放历史</Button>
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
        </div>
      {:else}
        <div class="advanced-body dim">正在读取…</div>
      {/if}
    </details>
  </section>
</div>

<style>
  /* 诊断值/路径/错误详情是用户可能要复制的内容，
     从全局 user-select:none 里豁免（diag 样式本体在全局 overlays.css） */
  .diag-grid,
  .advanced-body .dim,
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

  /* 分组说明：一句话说清"这组管什么"。以前靠块标题堆术语，
     现在每组标题下面有一句人话——术语要靠解释，不靠并列 */
  .section-heading p {
    margin: 3px 0 0;
    font-size: 11.5px;
    color: var(--text-2);
    max-width: 62ch;
  }
</style>
