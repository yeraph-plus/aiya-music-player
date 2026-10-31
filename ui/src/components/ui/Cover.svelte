<script lang="ts">
  /**
   * 封面：唯一的封面外观定义。
   *
   * 取代 `.cover` / `.cover.xs/.sm/.lg` / `.view-cover` / `.playlist-cover` 几处
   * 各自手写的尺寸与占位符（有的写 `♪`、有的写首字母、有的什么都不写留空）。
   *
   * 尺寸只有四档，新页面不要再造第五档 —— 档位少才能一眼看出层级：
   * xs 22（侧栏歌单行）/ sm 34（列表行）/ md 48（卡片）/ lg 92（详情页头）。
   *
   * `src` 为空时显示占位符。**不请求接口**：调用方用 `artworkUrl()` 拼好再传。
   */

  interface Props {
    src?: string | null;
    size?: 'xs' | 'sm' | 'md' | 'lg';
    alt?: string;
    /** 圆角档位：行内小图用 sm，大图用 lg */
    radius?: 'sm' | 'md' | 'lg';
  }

  let { src, size = 'sm', alt = '', radius }: Props = $props();
</script>

<div class="k-cover" class:xs={size === 'xs'} class:sm={size === 'sm'} class:md={size === 'md'} class:lg={size === 'lg'} class:round-lg={radius === 'lg'}>
  {#if src}
    <img {src} {alt} loading="lazy" decoding="async" />
  {:else}
    <span class="k-cover-ph" aria-hidden="true">♪</span>
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
