<script lang="ts">
  import type { Snippet } from 'svelte';
  import Cover from '../ui/Cover.svelte';
  import { t } from '../../lib/i18n.svelte';

  /**
   * 页内标题零件：**三层装饰文本**（装饰英文 kicker + 主标题 + 描述行）
   * 的全站唯一出处，直播/曲库/歌单/搜索/专辑/艺人/Beets/设置全部走这里。
   *
   * 与工具条分离且不并排：标题与操作按钮混排会迫使两者共享
   * padding 与对齐规则；页面按顺序书写两行标签，需要时可在其间插入其他内容。
   *
   * 不设 TopBar 层级：顶栏只承载全局元素（应用名、返回、搜索、窗口控制），
   * 页内标题随内容滚动，属于页面而非外壳。
   *
   * 三种形态由 `cover` 区分：
   * - 缺省（`undefined`）→ 纯文字标题（曲库 / 直播 / 设置）
   * - `null` → 有封面位但无图，显示占位符（歌单无封面）
   * - 字符串 → 92px 大封面（歌单 / 专辑）
   *
   * **横向 padding 归调用方**（`.page-pad`）：这样同一个零件既能放进
   * `.page-scroll`（直播页，已经带 22px），也能放在固定头位置（曲库，列表自己滚动）。
   *
   * 描述行（`subtitle`）写**产品陈述**——这一页是什么，不写操作方式
   * （操作是按钮与空态提示的事）。meta 只放统计（N 首 · 时长）。
   */

  interface Props {
    /** 标题上方的装饰英文，如「LIBRARY」「PLAYLIST」 */
    kicker?: string;
    title: string;
    /** 标题下一行说明（产品陈述，一句话） */
    subtitle?: string;
    /** 标题下的补充统计，如「共 12 首，43 分钟」。**与 subtitle 合并渲染在同一行**
     * （逗号分隔，不用间隔号），
     *  不另起一行。**是字符串不是 snippet**：
     *  snippet 的内容渲染在父组件作用域，这里的 scoped class 命中不了它 */
    meta?: string;
    cover?: string | null;
    /**
     * 无封面时的图标位（艺人详情页的 user 徽标）。与 `cover` 的关系：
     * cover 已给出 → Cover 组件；否则 icon 存在 → 92px 圆形图标块。
     */
    icon?: Snippet;
    /** 右侧操作区：tab 切换 / 子搜索框 / 按钮统一上下居中、同一控制高度 */
    actions?: Snippet;
  }

  let { kicker, title, subtitle, meta, cover, icon, actions }: Props = $props();
</script>

<header class="content-header" class:with-cover={cover !== undefined || icon !== undefined}>
  {#if cover !== undefined}
    <!-- 92px 大封面：尺寸/圆角/占位符/投影全在 Cover 的 lg 档里（kit 是唯一出处） -->
    <Cover src={cover || null} size="lg" />
  {:else if icon}
    <div class="ch-icon" aria-hidden="true">{@render icon()}</div>
  {/if}

  <div class="ch-text">
    {#if kicker}<span class="ch-kicker">{kicker}</span>{/if}
    <h1>{title}</h1>
    {#if subtitle || meta}
      <!-- 描述与统计合成一行：两层文本会把头部撑到四行，仪表盘页撑得太高 -->
      <p>
        {#if subtitle}{subtitle}{/if}{#if subtitle && meta}{t('library.metaSep')}{/if}{#if meta}<span class="ch-meta">{meta}</span>{/if}
      </p>
    {/if}
  </div>

  {#if actions}
    <div class="ch-actions">{@render actions()}</div>
  {/if}
</header>

<style>
  .content-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 0 18px;
    min-width: 0;
  }

  .ch-text {
    flex: 1;
    min-width: 0;
  }

  /* 装饰英文与正文样式在 pages.css 的 .ch-kicker（与首页区块的 .page-kicker 同源） */
  .ch-kicker {
    display: block;
  }

  /* 主标题：全站单一量级（26px）——三层结构里它就是视觉锚点，不再分档。
     不加粗：26px 的字号本身够重，粗体会和装饰英文抢对比 */
  h1 {
    margin: 2px 0 4px;
    font-size: var(--fs-display);
    font-weight: 400;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  p {
    margin: 0;
    font-size: var(--fs-base);
    color: var(--text-2);
  }

  .ch-meta {
    font-variant-numeric: tabular-nums;
  }

  /* 92px 圆形图标块：与 Cover.lg 同尺寸同投影，形状圆形（艺人徽标语义） */
  .ch-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 92px;
    height: 92px;
    border-radius: 50%;
    background: var(--surface-cover);
    color: var(--accent-text);
    box-shadow: var(--shadow-3);
  }

  /*
   * 右侧操作区：tab（TextTabs）/ 子搜索框 / 按钮三类混用时上下居中、
   * 高度统一吃到 --control-h。文本页签没有自带高度，这里给 nav 撑到
   * 同档（内部保持自己的 baseline 对齐），搜索框由调用方取同一变量。
   */
  .ch-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 6px;
    min-height: var(--control-h);
  }

  .ch-actions :global(.text-tabs.underline) {
    align-items: center;
    min-height: var(--control-h);
  }
</style>
