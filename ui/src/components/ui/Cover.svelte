<script lang="ts">
  /**
   * 封面：唯一的封面外观定义。
   *
   * 取代 `.cover` / `.cover.xs/.sm/.lg` / `.view-cover` / `.playlist-cover` 几处
   * 各自手写的尺寸与占位符（有的写 `♪`、有的写首字母、有的什么都不写留空）。
   *
   * 尺寸五档，新页面不要再造第六档 —— 档位少才能一眼看出层级：
   * xs 22（侧栏歌单行）/ sm 34（卡片、次要位）/ row 38（列表行，占满
   * cover-slot）/ md 48（卡片）/ lg 92（详情页头）。
   * row 档（发布前 UI 审查 P2-1）：此前三列表各自手写 38px img + ♪ 占位，
   * 与本组件的 34px sm 档同名不同值——收编后占位符语言也统一（失败行
   * 用 glyph="!"，骨架行 glyph=""）。
   *
   * `src` 为空时显示占位符。**不请求接口**：调用方用 `artworkUrl()` 拼好再传。
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
