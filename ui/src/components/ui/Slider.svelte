<script lang="ts">
  import { Slider } from 'bits-ui';

  /**
   * 滑块：进度、音量与倍速三处共用。
   *
   * 内核为 `bits-ui/Slider`（单拇指），提供指针捕获、Home/End、←/→ 步进与
   * `aria-valuenow/min/max`。
   *
   * ## `oninput` 与 `oncommit` 的分工（属正确性要求，非可选优化）
   *
   * - `oninput` 在每次指针移动时触发 → 音量使用此回调（用户拖动时期待听到变化）
   * - `oncommit` 在松手或键盘操作结束时触发 → 进度条 seek 必须使用此回调；
   *   拖动中逐帧 seek 会使 Rust 侧反复 flush 输出缓冲，实测产生咔哒声
   *
   * 两者均可选，可只传其一。
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

  /* 竖向（EQ 频段）：轨道竖画，宽度即厚度，高度由调用方指定。
     min-width 须归零：横向档的 min-width:40 会把 22px 的竖轨撑到 40px，导致
     EQ 抽屉出现横向滚动 */
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
