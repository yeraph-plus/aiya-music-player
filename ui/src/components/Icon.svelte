<script lang="ts">
  import type { Component } from 'svelte';
  import {
    Folder,
    GripVertical,
    Heart,
    ListVideo,
    Moon,
    Pause,
    Pencil,
    Play,
    Repeat,
    Repeat1,
    RepeatOff,
    RotateCw,
    Shuffle,
    SkipBack,
    SkipForward,
    Sun,
    Trash,
    Volume2,
    VolumeX,
    X,
  } from '@lucide/svelte';

  /**
   * 图标注册表：语义名 → lucide 组件。
   *
   * 表内 20 个语义名即当前全部图标。图标来源只有 `@lucide/svelte`，新增图标在表内
   * 注册一行；不手写 SVG path——混入手写 path 会使同一界面的 strokeWidth 出现多个
   * 取值。
   *
   * 无使用方的语义名不登记（`search` / `clock` / `flame` / `list` / `settings` /
   * `lyrics` / `check` / `more` / `spectrum` 当前无消费方），避免让读者以为功能仍在。
   *
   * 线宽与尺寸约定：本表 stroke = 1.8、默认 size = 16；组件直连 `@lucide/svelte`
   * 时沿用这两档，强调值 2.4 仅用于排序与开合箭头，尺寸取 12 / 14 / 16 三档。
   * 文本字符（↑ ▶ ✓）不作图标：字体渲染的粗细与基线随系统字体变化，与 lucide
   * 的 24 栅格不对应，同一行内会出现视觉错位。
   */
  const ICONS = {
    play: Play,
    pause: Pause,
    prev: SkipBack,
    next: SkipForward,
    shuffle: Shuffle,
    repeat: Repeat,
    repeatOne: Repeat1,
    sequential: RepeatOff,
    volume: Volume2,
    mute: VolumeX,
    queue: ListVideo,
    close: X,
    folder: Folder,
    heart: Heart,
    grip: GripVertical,
    trash: Trash,
    edit: Pencil,
    refresh: RotateCw,
    sun: Sun,
    moon: Moon,
  } satisfies Record<string, Component>;

  type IconName = keyof typeof ICONS;

  /**
   * 实心图标集合：播放条上这五个为实心形状，lucide 全部为描边图形。fill 的翻转
   * 规则在 base.css 的 `.icon-solid`：scoped 样式不影响子组件渲染的 `<svg>`，只能
   * 走全局选择器。
   */
  const SOLID = new Set<IconName>(['play', 'pause', 'prev', 'next', 'queue']);

  /** 统一线宽。lucide 默认 2 在 16px 下偏粗；大号展示图标自己传更细的值 */
  const STROKE = 1.8;

  interface Props {
    name: IconName;
    size?: number;
  }

  let { name, size = 16 }: Props = $props();

  const Glyph = $derived(ICONS[name]);
  const solid = $derived(SOLID.has(name));
</script>

<Glyph size={size} strokeWidth={STROKE} class={solid ? 'icon-solid' : undefined} aria-hidden="true" />
