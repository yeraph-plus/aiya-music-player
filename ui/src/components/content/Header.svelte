<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * 页内标题零件。
   *
   * 2026-10-26 壳体收口把它从「视图头」里拆出来（docs/24 §3.1 修订 v2）：
   *
   * - **和工具条不在一行、不组合**。旧实现把标题、统计、四个操作按钮塞进一个
   *   `.view-head`，首页又另有一份 `.page-heading`，两套形状各写一遍 padding；
   *   设置页更极端 —— 标题和 18 个锚点 chip 挤在同一个 `header` 里，active 状态
   *   只能靠滚动反查写回 `.chip.on`。现在标题归这里、工具条归 `Toolbar`，
   *   页面自己按顺序写两行标签，中间想插扫描进度条就插。
   * - **不设 TopBar 这一级**。顶栏只放全局的（应用名/返回/搜索/窗口控制），
   *   页内标题属于页面自己 —— 它随内容滚动，不是外壳。
   *
   * 三种形态由 `cover` 区分：
   * - 缺省（`undefined`）→ 纯文字标题（首页 / 曲库 / 统计）
   * - `null` → 有封面位但没图，显示占位符（歌单无封面）
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
    cover?: string | null;
    /** 右侧操作按钮组 */
    actions?: Snippet;
  }

  let { kicker, title, subtitle, meta, cover, actions }: Props = $props();
</script>

<header class="content-header" class:with-cover={cover !== undefined}>
  {#if cover !== undefined}
    <div class="ch-cover">
      {#if cover}
        <img src={cover} alt="" />
      {:else}
        ♪
      {/if}
    </div>
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

  .ch-cover {
    flex: none;
    display: grid;
    place-items: center;
    width: 92px;
    height: 92px;
    border-radius: 12px;
    overflow: hidden;
    background: var(--surface-cover);
    color: var(--text-3);
    font-size: 30px;
    box-shadow: 0 12px 30px var(--shadow);
  }

  .ch-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .ch-text {
    flex: 1;
    min-width: 0;
  }

  h1 {
    margin: 3px 0 4px;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 有封面时标题更大一档：封面本来就是视觉重心 */
  .with-cover h1 {
    font-size: 26px;
  }

  p {
    margin: 0;
    font-size: 12px;
    color: var(--text-2);
  }

  .ch-meta {
    margin-top: 4px;
    font-size: 11.5px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .ch-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
</style>
