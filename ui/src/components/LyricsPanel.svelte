<script lang="ts">
  import type { Lyrics } from '../lib/ipc';
  import { findActiveLine } from '../lib/lyrics';
  import { anchorAt, projectPosition } from '../lib/lyrics-clock';

  interface Props {
    lyrics: Lyrics | null;
    loading: boolean;
    trackTitle: string | null;
    positionMs: number;
    durationMs: number;
    playing: boolean;
    onseek: (ms: number) => void;
  }

  let { lyrics, loading, trackTitle, positionMs, durationMs, playing, onseek }: Props = $props();

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
  <!-- 无头部（T4d）、无底部提示条（用户反馈：编码/跳转提示去掉）——
       正在播放页的页面语境已经说明这里是什么；唯一消费方是正在播放页右列 -->
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
    {:else}
      <!-- 上下各半高空白占位（用户拍板）：歌词区上下居中——激活行跟随滚动
           本来就把激活行滚到容器中线，占位让**第一句之前 / 最后一句之后**
           也有半屏余量，首尾句同样能滚到中线；占位用 flex-basis 50% 直接
           由容器高度算出，不需要 JS 测量。 -->
      <div class="lyrics-pad" aria-hidden="true"></div>
      {#if !lyrics.synced}
        <!-- 纯文本歌词没有逐行 seek 语义，整块放开选择（docs/19 §5）；
             同步歌词的行是 button（点=seek），文本选择让位给点击手势 -->
        <div
          class="lyrics-lines plain selectable"
          aria-label={`歌词（纯文本）：《${trackTitle ?? '当前曲目'}》`}
        >
          {#each lines as line, index (index)}
            <div class="lyric-line">{line.text}</div>
          {/each}
        </div>
      {:else}
        <div class="lyrics-lines" aria-label={`同步歌词：《${trackTitle ?? '当前曲目'}》，点击或回车某一行跳转到该句`}>
          {#each lines as line, index (index)}
            <button
              class="lyric-line"
              class:active={index === activeIndex}
              class:past={index < activeIndex}
              onclick={() => seekTo(line.timeMs)}
              onkeydown={(event) => {
                // 原生 button 自带 Enter 触发，Space 在部分浏览器只滚动不点击，这里补齐
                if (event.key === ' ') {
                  event.preventDefault();
                  seekTo(line.timeMs);
                }
              }}
              title="跳到这一句"
            >
              {line.text || '·'}
            </button>
          {/each}
        </div>
      {/if}
      <div class="lyrics-pad" aria-hidden="true"></div>
    {/if}
  </div>
</aside>
