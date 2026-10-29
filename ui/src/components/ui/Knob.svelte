<script lang="ts">
  /**
   * 旋钮（docs/39：前置增益/低音/高音的实体音响观感）。
   *
   * 交互：按住**横向**拖动（150px = 全量程，右增左减——旋钮很密时
   * 竖拖会撞上下邻元素，横拖在纵排面板里更稳）、滚轮微调（±step）、
   * 键盘（方向键 ±step、Home/End 两端）。270° 扫角，SVG 弧线画轨道与
   * 当前值弧，指针线随值转动。
   *
   * 与 Slider 同约定：**emit 前先吸附 step 网格**——不吸附会让 clamp 回写
   * 与显示值打架（0.5 步进下出现 3.333 之类的鬼值）。
   *
   * 无障碍：role="slider" + aria-valuemin/max/now + 方向键，与 bits-ui
   * Slider 的键盘行为对齐。
   */
  interface Props {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    disabled?: boolean;
    /** 无障碍名称，如「前置增益」 */
    label: string;
    oninput?: (value: number) => void;
  }

  let { value, min = 0, max = 100, step = 1, disabled = false, label, oninput }: Props = $props();

  const SWEEP = 270; // 扫角（度）：-135°..+135°，指针朝上为 min/max 中点
  const SIZE = 88;
  const R = 36; // 弧线半径
  const DRAG_RANGE = 150; // 横向拖动满量程的像素距离

  /** 归一化位置 0..1 → 当前角度（-135°..+135°，0° 朝上） */
  const angle = $derived(-SWEEP / 2 + ((value - min) / (max - min || 1)) * SWEEP);

  /** 角度 → SVG 坐标（y 向下，0° 在正上方，顺时针为正） */
  function polar(deg: number, radius: number = R): [number, number] {
    const rad = (deg * Math.PI) / 180;
    const c = SIZE / 2;
    return [c + radius * Math.sin(rad), c - radius * Math.cos(rad)];
  }

  function arcPath(fromDeg: number, toDeg: number): string {
    const [x1, y1] = polar(fromDeg);
    const [x2, y2] = polar(toDeg);
    const large = Math.abs(toDeg - fromDeg) > 180 ? 1 : 0;
    const sweepFlag = toDeg > fromDeg ? 1 : 0;
    return `M ${x1.toFixed(2)} ${y1.toFixed(2)} A ${R} ${R} 0 ${large} ${sweepFlag} ${x2.toFixed(2)} ${y2.toFixed(2)}`;
  }

  const trackPath = arcPath(-SWEEP / 2, SWEEP / 2);
  // 值弧：从 0°（中点）画到当前角，方向随正负
  const valuePath = $derived(angle >= 0 ? arcPath(0, angle) : arcPath(angle, 0));
  // 指针线：内半径 16 → 外半径 28
  const pinOuter = $derived(polar(angle, 28));
  const pinInner = $derived(polar(angle, 16));

  function snap(v: number): number {
    const snapped = Math.round(v / step) * step;
    return Math.min(max, Math.max(min, snapped));
  }

  let dragging = $state(false);
  let dragStartX = 0;
  let dragStartValue = 0;

  function onpointerdown(e: PointerEvent): void {
    if (disabled) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartValue = value;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging || disabled) return;
    const dx = e.clientX - dragStartX; // 向右拖 = 增
    const next = snap(dragStartValue + (dx / DRAG_RANGE) * (max - min));
    if (next !== value) oninput?.(next);
  }

  function onpointerup(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }

  function onwheel(e: WheelEvent): void {
    if (disabled) return;
    e.preventDefault();
    const next = snap(value + (e.deltaY < 0 ? step : -step));
    if (next !== value) oninput?.(next);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (disabled) return;
    const jumps: Record<string, number> = {
      ArrowUp: step,
      ArrowRight: step,
      ArrowDown: -step,
      ArrowLeft: -step,
    };
    let next: number | undefined;
    if (e.key in jumps) next = snap(value + jumps[e.key]);
    else if (e.key === 'Home') next = min;
    else if (e.key === 'End') next = max;
    if (next === undefined) return;
    e.preventDefault();
    if (next !== value) oninput?.(next);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -- role=slider 需要 tabindex 与键盘事件，交互语义在脚本层 -->
<div
  class="knob"
  class:disabled
  role="slider"
  tabindex={disabled ? -1 : 0}
  aria-label={label}
  aria-valuemin={min}
  aria-valuemax={max}
  aria-valuenow={value}
  aria-disabled={disabled}
  onpointerdown={onpointerdown}
  onpointermove={onpointermove}
  onpointerup={onpointerup}
  onpointercancel={() => (dragging = false)}
  onwheel={onwheel}
  onkeydown={onkeydown}
>
  <svg width={SIZE} height={SIZE} viewBox="0 0 {SIZE} {SIZE}">
    <circle class="body" cx={SIZE / 2} cy={SIZE / 2} r={R - 5} />
    <path class="track" d={trackPath} />
    {#if Math.abs(angle) > 0.5}
      <path class="value" d={valuePath} />
    {/if}
    <line class="pin" x1={pinInner[0]} y1={pinInner[1]} x2={pinOuter[0]} y2={pinOuter[1]} />
  </svg>
</div>

<style>
  .knob {
    width: 88px;
    height: 88px;
    border-radius: 50%;
    cursor: ew-resize;
    touch-action: none;
    user-select: none;
    outline-offset: 3px;
  }

  .knob:global(:focus-visible) {
    outline: 2px solid var(--accent);
  }

  .knob.disabled {
    opacity: 0.45;
    cursor: default;
  }

  svg {
    display: block;
  }

  .body {
    fill: var(--surface-1);
    stroke: var(--line);
    stroke-width: 1;
  }

  .track {
    fill: none;
    stroke: var(--surface-hover);
    stroke-width: 5;
    stroke-linecap: round;
  }

  .value {
    fill: none;
    stroke: var(--accent);
    stroke-width: 5;
    stroke-linecap: round;
  }

  .pin {
    stroke: var(--text-1);
    stroke-width: 3;
    stroke-linecap: round;
  }
</style>
