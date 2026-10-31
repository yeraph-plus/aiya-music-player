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
   * 语义名 → lucide 图标。
   *
   * 2026-10-26 之前这里是 29 条手写 SVG path（169 行），而另外 9 个组件
   * 直接 import lucide —— 同一套界面两种图标来源，strokeWidth 还有 1.3/1.7/1.8
   * 三种。现在只剩这张表：**要用新图标就加一行**，不要回来手画 path。
   *
   * 只保留实际用到的 20 个。历史上有、现在没有的（search/clock/flame/list/
   * settings/lyrics/check/more/spectrum）已删 —— 需要时一行就够，留着没人用的
   * 映射只会让人以为功能还在。
   *
   * 直连 lucide 的约定（发布前 UI 审查 P2-2）：本表 stroke=1.8、默认 size=16；
   * 组件里直连 `@lucide/svelte` 时沿用这两档（特殊强调 2.4 只给排序/开合箭头），
   * 尺寸用 12/14/16 三档——不要再出现 15 种离散值。文本字符（↑ ▶ ✓）不当
   * 图标用：字体渲染粗细/基线随系统走，与 lucide 不同栅格。
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
   * 需要**实心**的图标：播放条上这五个是实心形状（旧手绘就是 fill），
   * lucide 全是描边图形。翻转 fill 的规则在 base.css 的 `.icon-solid`
   * （scoped 样式够不到子组件渲染出来的 `<svg>`，只能全局）。
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
