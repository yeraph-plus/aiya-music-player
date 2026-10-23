<script lang="ts">
  import type { Lyrics } from '@aiya/contracts';
  import { findActiveLine } from '@aiya/contracts';
  import { anchorAt, projectPosition } from '../lib/lyrics-clock';
  import Icon from './Icon.svelte';

  interface Props {
    lyrics: Lyrics | null;
    loading: boolean;
    trackTitle: string | null;
    positionMs: number;
    durationMs: number;
    playing: boolean;
    onseek: (ms: number) => void;
    onclose: () => void;
  }

  let { lyrics, loading, trackTitle, positionMs, durationMs, playing, onseek, onclose }: Props =
    $props();

  let scroller = $state<HTMLDivElement | null>(null);
  let userScrolledAt = 0;

  /*
   * 高亮位置自己走时钟，不等主进程的状态广播。
   *
   * 位置上报是 500ms 一次（进度条够用），直接拿它算高亮，每句歌词最多会晚
   * 半秒才对上 —— 验收要求 < 200ms，靠上报频率是做不到的。
   * 所以：每次收到新位置就重新"对时"，播放中再用 rAF 按真实时间外推，
   * 误差落在一帧（~16ms）内；暂停时停表，不虚走。
   */
  let clockMs = $state(0);
  let anchor = anchorAt(0, 0);

  function reanchor(ms: number): void {
    anchor = anchorAt(ms, performance.now());
    clockMs = anchor.position;
  }

  // 位置上报或切歌时重新对时
  $effect(() => {
    reanchor(positionMs);
  });

  $effect(() => {
    if (!playing) return;
    let frame = 0;
    const tick = (): void => {
      clockMs = projectPosition(anchor, performance.now(), durationMs);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  });

  function seekTo(ms: number): void {
    // 先本地跳，下一帧高亮就落到新位置，不用等 500ms 后的上报
    reanchor(ms);
    onseek(ms);
  }

  const lines = $derived(lyrics?.lines ?? []);
  const activeIndex = $derived(findActiveLine(lines, clockMs));

  // 用户手动滚过之后 2 秒内不抢滚动条。只认真实输入事件（滚轮/触摸/按下），
  // 不认 scroll —— 程序化 smooth 滚动也会触发 scroll，会把跟随自己抑制掉。
  // touch/pointer 挂 window 上（容器上的静态元素交互会报 a11y 警告），
  // 但只在目标落在歌词容器内时生效；点歌词行按钮是"跳转"不是"翻走"，不抑制。
  function markUserScroll(event: Event): void {
    if (!scroller || !(event.target instanceof Node) || !scroller.contains(event.target)) return;
    if (event.target instanceof Element && event.target.closest('button')) return;
    userScrolledAt = Date.now();
  }

  $effect(() => {
    const index = activeIndex;
    if (!scroller || index < 0) return;
    if (Date.now() - userScrolledAt < 2000) return;
    const active = scroller.querySelector<HTMLElement>('.lyric-line.active');
    if (!active) return;
    const target =
      active.offsetTop - scroller.clientHeight / 2 + active.clientHeight / 2;
    scroller.scrollTo({ top: Math.max(0, target), behavior: 'smooth' });
  });
</script>

<svelte:window ontouchstart={markUserScroll} onpointerdown={markUserScroll} />

<aside class="lyrics-panel" aria-label="歌词">
  <header>
    <strong>歌词</strong>
    <span class="dim">{lyrics?.found ? (lyrics.synced ? '同步歌词' : '纯文本') : '暂无'}</span>
    <button class="icon-btn" onclick={onclose} aria-label="关闭歌词"><Icon name="close" size={14} /></button>
  </header>

  <div
    class="lyrics-body"
    bind:this={scroller}
    onwheel={() => (userScrolledAt = Date.now())}
  >
    {#if loading}
      <div class="lyrics-empty">正在查找歌词…</div>
    {:else if !lyrics?.found}
      <div class="lyrics-empty">
        《{trackTitle ?? '当前曲目'}》没有歌词<br />
        <span class="dim">在同目录放一个同名的 .lrc 文件即可显示</span>
      </div>
    {:else if !lyrics.synced}
      <div class="lyrics-lines plain">
        {#each lines as line, index (index)}
          <div class="lyric-line">{line.text}</div>
        {/each}
      </div>
    {:else}
      <div class="lyrics-lines">
        {#each lines as line, index (index)}
          <button
            class="lyric-line"
            class:active={index === activeIndex}
            class:past={index < activeIndex}
            onclick={() => seekTo(line.timeMs)}
            title="跳到这一句"
          >
            {line.text || '·'}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  {#if lyrics?.found}
    <footer class="lyrics-foot">
      {lyrics.encoding ? `编码 ${lyrics.encoding}` : ''}
      {lyrics.offsetMs !== 0 ? ` · 偏移 ${lyrics.offsetMs > 0 ? '+' : ''}${lyrics.offsetMs}ms` : ''}
      <span class="dim">点击歌词可跳转</span>
    </footer>
  {/if}
</aside>
