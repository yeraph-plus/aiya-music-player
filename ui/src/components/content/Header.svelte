<script lang="ts">
  import type { Snippet } from 'svelte';
  import Cover from '../ui/Cover.svelte';

  /**
   * 页内标题零件。
   *
   * 与工具条分离且不并排：标题与操作按钮混排会迫使两者共享
   * padding 与对齐规则；页面按顺序书写两行标签，需要时可在其间插入其他内容。
   *
   * 不设 TopBar 层级：顶栏只承载全局元素（应用名、返回、搜索、窗口控制），
   * 页内标题随内容滚动，属于页面而非外壳。
   *
   * 三种形态由 `cover` 区分：
   * - 缺省（`undefined`）→ 纯文字标题（首页 / 曲库 / 统计）
   * - `null` → 有封面位但无图，显示占位符（歌单无封面）
   * - 字符串 → 92px 大封面（歌单 / 专辑）
   *
   * **横向 padding 归调用方**（`.page-pad`）：这样同一个零件既能放进
   * `.page-scroll`（首页，已经带 22px），也能放在固定头位置（曲库，列表自己滚动）。
   */

  interface Props {
    /** 标题上方的小字，如「播放列表」「YOUR LIBRARY」 */
    kicker?: string;
    title: string;
    /** 标题下一行说明（一句话） */
    subtitle?: string;
    /** 标题下的补充信息，如「12 首 · 43 分钟」。**是字符串不是 snippet**：
     *  snippet 的内容渲染在父组件作用域，这里的 scoped class 命中不了它 */
    meta?: string;
    /**
     * 大标题形态：列表页不使用 kicker 与描述行构成的三层结构，只保留大标题
     * 与 meta 次要统计。封面页（歌单、详情）不使用该形态——封面已是视觉重心，
     * 标题按 26px 配平。
     */
    big?: boolean;
    cover?: string | null;
    /**
     * 无封面时的图标位（艺人详情页的 user 徽标）。与 `cover` 的关系：
     * cover 已给出 → Cover 组件；否则 icon 存在 → 92px 圆形图标块。
     * 有图标位时标题同样升到 26px（with-cover 类，视觉权重与封面位一致）。
     */
    icon?: Snippet;
    /** 右侧操作按钮组 */
    actions?: Snippet;
  }

  let { kicker, title, subtitle, meta, big = false, cover, icon, actions }: Props = $props();
</script>

<header class="content-header" class:big class:with-cover={cover !== undefined || icon !== undefined}>
  {#if cover !== undefined}
    <!-- 92px 大封面：尺寸/圆角/占位符/投影全在 Cover 的 lg 档里（kit 是唯一出处） -->
    <Cover src={cover || null} size="lg" />
  {:else if icon}
    <div class="ch-icon" aria-hidden="true">{@render icon()}</div>
  {/if}

  <div class="ch-text">
    {#if kicker}<span class="ch-kicker">{kicker}</span>{/if}
    <h1>{title}</h1>
    {#if subtitle}<p>{subtitle}</p>{/if}
    {#if meta}<div class="ch-meta">{meta}</div>{/if}
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

  h1 {
    margin: 3px 0 4px;
    font-size: var(--fs-2xl);
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 有封面时标题更大一档：封面本来就是视觉重心 */
  .with-cover h1 {
    font-size: var(--fs-display);
  }

  /* 大标题形态（列表页）：三层结构取消后唯一的视觉锚点。字号 26px，与封面页
     标题同档，使全站标题保持单一量级 */
  .big h1 {
    margin-top: 0;
    font-size: var(--fs-display);
    letter-spacing: -0.02em;
  }

  .big .ch-meta {
    margin-top: 6px;
    font-size: var(--fs-base);
  }

  p {
    margin: 0;
    font-size: var(--fs-base);
    color: var(--text-2);
  }

  .ch-meta {
    margin-top: 4px;
    font-size: var(--fs-sm);
    color: var(--text-2);
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

  .ch-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
</style>
