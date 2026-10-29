<script lang="ts">
  import { Clock3, Database, Disc3, ListMusic, Mic2, TriangleAlert } from '@lucide/svelte';
  import BeetsSection from '../components/BeetsSection.svelte';
  import LibraryTools from '../components/LibraryTools.svelte';
  import Header from '../components/content/Header.svelte';
  import { formatBytes, formatDurationLong } from '../lib/format';
  import { library } from '../stores/library.svelte';

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
   * 版本/数据目录/最近错误等识别与排障信息在设置页「关于」tab
   * （2026-10-28 用户拍板迁出——它描述的是整个应用，不是"入库"这件事）。
   *
   * 下放出去的东西（不要去这里找）：
   * - 版本 / 平台 / Schema / 数据目录 / 最近错误 → 设置 → 关于
   * - 快捷键冲突 → 设置 → 快捷键（那边本来就有，这里是重复的第二份）
   * - 媒体控制 / 防休眠 / 托盘 → 设置 → 系统（各自开关旁边一行状态）
   * - 自动刷新 → 已删展示（静默管线，用户拍板那行文本没用；管线本身还在跑）
   * - 媒体流 / 最近查询 → 已删（前者是协议层泄漏自检，后者是 SQL 性能排查，
   *   两者都属于开发者视角；真要排查看日志）
   */
  /* 曲库统计（2026-10-28 从侧栏底部迁来）：统计跟它描述的对象走——
     看数字的人正要扫描/整理库，数字放这比放在侧栏常驻更对位。 */
  const stats = $derived(library.stats);
</script>

<div class="page-scroll tools-page">
  <Header title="扫描入库" big />

  <section class="tools-section" id="tools-library">
    <div class="section-heading">
      <h2>曲库</h2>
    </div>
    {#if stats}
      <div class="lib-stats">
        <!-- 统计卡：图标 + 大数字 + 标签。数字 tabular 不抖；图标用中性描边件，
             不抢数字的戏。索引大小不是"音乐文件多大"，是 SQLite 文件自己的体积 -->
        <div class="stat-card">
          <span class="stat-icon"><ListMusic size={17} /></span>
          <div class="stat-text"><b>{stats.tracks}</b><span>曲目</span></div>
        </div>
        <div class="stat-card">
          <span class="stat-icon"><Disc3 size={17} /></span>
          <div class="stat-text"><b>{stats.albums}</b><span>专辑</span></div>
        </div>
        <div class="stat-card">
          <span class="stat-icon"><Mic2 size={17} /></span>
          <div class="stat-text"><b>{stats.artists}</b><span>艺人</span></div>
        </div>
        <div class="stat-card">
          <span class="stat-icon"><Clock3 size={17} /></span>
          <div class="stat-text"><b>{formatDurationLong(stats.totalDurationMs)}</b><span>总时长</span></div>
        </div>
        <div class="stat-card">
          <span class="stat-icon"><Database size={17} /></span>
          <div class="stat-text"><b>{formatBytes(stats.dbBytes)}</b><span>索引大小</span></div>
        </div>
        {#if stats.missingLocations > 0}
          <div class="stat-card warn">
            <span class="stat-icon"><TriangleAlert size={17} /></span>
            <div class="stat-text"><b>{stats.missingLocations}</b><span>位置缺失</span></div>
          </div>
        {/if}
      </div>
    {/if}
    <LibraryTools />
  </section>

  <section class="tools-section" id="tools-beets">
    <div class="section-heading">
      <h2>整理库</h2>
    </div>
    <BeetsSection />
  </section>
</div>

<style>
  .tools-section {
    margin-top: 26px;
  }

  /* 统计卡（2026-10-28 七轮拍板：放大字号 + 图标 + 卡片感）：
     左侧图标座（accent 弱底色圆角方块），右侧大数字 + 小标签竖排 */
  .lib-stats {
    display: flex;
    align-items: stretch;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }

  .stat-card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 18px 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    min-width: 118px;
  }

  .stat-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent-text);
  }

  .stat-card.warn .stat-icon {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }

  .stat-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .stat-text b {
    font-size: 21px;
    font-weight: 400; /* 用户拍板：数字不加粗，靠字号本身立住 */
    line-height: 1.15;
    color: var(--text-1);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .stat-text span {
    font-size: 11.5px;
    color: var(--text-2);
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
