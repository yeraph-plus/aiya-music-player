<script lang="ts">
  import { Slider } from 'bits-ui';

  /**
   * 滑块：进度 / 音量 / 倍速三处共用。
   *
   * 内核 `bits-ui/Slider`（单拇指）：指针捕获、`Home/End`、`←/→` 步进、
   * `aria-valuenow/min/max`。取代三个 `<input type="range">`（它们各自写了
   * 一套 `-webkit-slider-thumb` 伪元素样式，亮暗主题下还不一致）。
   *
   * ## `oninput` 与 `oncommit` 的分工（不是可选优化，是正确性）
   *
   * - `oninput` 每次指针移动都触发 → **音量**用它（用户拖着就想听到变化）
   * - `oncommit` 只在松手/键盘结束时触发 → **进度条 seek 必须用它**
   *   拖动中每帧 seek 会让 Rust 侧反复 flush 输出缓冲，实测有咔哒声
   *
   * 两者都可选；只传一个也行。
   */

  interface Props {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    disabled?: boolean;
    /** 无障碍名称，如「播放进度」「音量」 */
    label: string;
    oninput?: (value: number) => void;
    oncommit?: (value: number) => void;
    /** 轨道宽度：进度条 100%，音量固定 84px */
    width?: string;
    /**
     * 竖向（B5 EQ 频段滑杆）：值从下往上增（与横向左→右一致）。
     * 竖向时 `width` 是轨道的**厚度**，高度由调用方的样式决定。
     */
    orientation?: 'horizontal' | 'vertical';
  }

  let {
    value,
    min = 0,
    max = 100,
    step = 1,
    disabled = false,
    label,
    oninput,
    oncommit,
    width,
    orientation = 'horizontal',
  }: Props = $props();
</script>

<Slider.Root
  type="single"
  {value}
  {min}
  {max}
  {step}
  {disabled}
  {orientation}
  aria-label={label}
  onValueChange={(next) => oninput?.(next)}
  onValueCommit={(next) => oncommit?.(next)}
  class={`k-slider${orientation === 'vertical' ? ' vert' : ''}`}
  style={width ? `width:${width}` : undefined}
>
  <Slider.Range class="k-slider-range" />
  <Slider.Thumb class="k-slider-thumb" index={0} aria-label={label} />
</Slider.Root>

<style>
  :global(.k-slider) {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1;
    min-width: 40px;
    height: 16px;
    touch-action: none;
    cursor: pointer;
  }

  /* 轨道：用 Root 的 ::before 画，Range 只负责已播放段 */
  :global(.k-slider)::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    height: 4px;
    border-radius: var(--radius-pill);
    background: var(--surface-hover);
  }

  :global(.k-slider-range) {
    position: absolute;
    height: 4px;
    border-radius: var(--radius-pill);
    background: var(--accent);
  }

  :global(.k-slider-thumb) {
    position: relative;
    display: block;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--play-bg);
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease);
  }

  :global(.k-slider:hover .k-slider-thumb),
  :global(.k-slider-thumb:focus-visible) {
    transform: scale(1.18);
  }

  :global(.k-slider-thumb:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  :global(.k-slider[data-disabled]) {
    opacity: var(--disabled-opacity);
    cursor: default;
  }

  /* 竖向（EQ 频段）：轨道竖画，宽度即厚度，高度由调用方给。
     min-width 必须归零——横向档的 min-width:40 会把 22px 的竖轨撑到 40px，
     溢出波段列（2026-10-28 EQ 抽屉横滚的真因之一） */
  :global(.k-slider.vert) {
    width: 22px;
    min-width: 0;
    height: 100%;
    flex: none;
    justify-content: center;
  }

  :global(.k-slider.vert)::before {
    left: 50%;
    right: auto;
    top: 0;
    bottom: 0;
    width: 4px;
    height: auto;
    transform: translateX(-50%);
  }

  :global(.k-slider.vert .k-slider-range) {
    left: 50%;
    right: auto;
    top: auto;
    bottom: 0;
    width: 4px;
    height: auto;
    transform: translateX(-50%);
  }
</style>
