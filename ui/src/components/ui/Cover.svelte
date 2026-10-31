<script lang="ts">
  /**
   * 封面：唯一的封面外观定义。
   *
   * 全部尺寸与占位符由此统一，各调用点不自行绘制（占位符曾混用 `♪`、首字母
   * 与空白）。
   *
   * 尺寸五档，不新增第六档 —— 档位少才能快速判断层级：
   * xs 22（侧栏歌单行）/ sm 34（卡片、次要位）/ row 38（列表行，占满
   * cover-slot）/ md 48（卡片）/ lg 92（详情页头）。
   * row 档为列表行专用：各列表自行绘制的 38px 图与 `♪` 占位已合并到该档，占位符
   * 语言随之统一（加载失败用 glyph="!"，骨架行 glyph=""）。
   *
   * `src` 为空时显示占位符。本组件不发起请求：调用方以 `artworkUrl()` 构造后传入。
   */

  interface Props {
    src?: string | null;
    size?: 'xs' | 'sm' | 'row' | 'md' | 'lg';
    alt?: string;
    /** 圆角档位：行内小图用 sm，大图用 lg */
    radius?: 'sm' | 'md' | 'lg';
    /** 占位符字符（无封面时）：默认 ♪；解析失败行用 !，骨架行留空 */
    glyph?: string;
  }

  let { src, size = 'sm', alt = '', radius, glyph = '♪' }: Props = $props();
</script>

<div class="k-cover" class:xs={size === 'xs'} class:sm={size === 'sm'} class:sz-row={size === 'row'} class:md={size === 'md'} class:lg={size === 'lg'} class:round-lg={radius === 'lg'}>
  {#if src}
    <img {src} {alt} loading="lazy" decoding="async" />
  {:else}
    <span class="k-cover-ph" aria-hidden="true">{glyph}</span>
  {/if}
</div>

<style>
  .k-cover {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--surface-cover);
    color: var(--text-3);
    font-size: var(--fs-md);
  }

  .k-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .k-cover.xs {
    width: 22px;
    height: 22px;
    border-radius: 4px;
    font-size: var(--fs-2xs);
  }

  .k-cover.sz-row {
    width: 38px;
    height: 38px;
    border-radius: 6px;
  }

  .k-cover.md {
    width: 48px;
    height: 48px;
    border-radius: var(--radius-md);
    font-size: var(--fs-xl);
  }

  .k-cover.lg {
    width: 92px;
    height: 92px;
    border-radius: var(--radius-lg);
    font-size: var(--fs-hero);
    box-shadow: var(--shadow-3);
  }

  .k-cover.round-lg {
    border-radius: var(--radius-lg);
  }

  .k-cover-ph {
    user-select: none;
  }
</style>
