<script lang="ts">
  type IconName =
    | 'play'
    | 'pause'
    | 'prev'
    | 'next'
    | 'shuffle'
    | 'repeat'
    | 'repeatOne'
    | 'queue'
    | 'volume'
    | 'mute'
    | 'close'
    | 'search'
    | 'folder'
    | 'clock'
    | 'flame'
    | 'list'
    | 'settings'
    | 'plus'
    | 'grip'
    | 'lyrics'
    | 'check'
    | 'trash'
    | 'edit'
    | 'more'
    | 'refresh'
    | 'sun'
    | 'moon'
    | 'spectrum';

  interface Shape {
    d: string;
    fill?: boolean;
  }

  const ICONS: Record<IconName, Shape[]> = {
    play: [{ d: 'M8.5 5.2v13.6L19 12z', fill: true }],
    pause: [
      { d: 'M9 5h2.3v14H9z', fill: true },
      { d: 'M12.7 5H15v14h-2.3z', fill: true },
    ],
    prev: [{ d: 'M7 5v14' }, { d: 'M19 5.6v12.8L9.6 12z', fill: true }],
    next: [{ d: 'M17 5v14' }, { d: 'M5 5.6v12.8L14.4 12z', fill: true }],
    shuffle: [
      { d: 'M17 3.2l3 2.8-3 2.8' },
      { d: 'M17 15.2l3 2.8-3 2.8' },
      { d: 'M4 7h3.4c1.6 0 2.6.9 3.5 2.2l3 4.6c.9 1.3 1.9 2.2 3.5 2.2H20' },
      { d: 'M4 17h3.4c1.6 0 2.6-.9 3.5-2.2M20 7h-2.6c-1.6 0-2.6.9-3.5 2.2' },
    ],
    repeat: [
      { d: 'M17 3.2l3 2.8-3 2.8' },
      { d: 'M20 6H7.2A3.2 3.2 0 0 0 4 9.2v1.6' },
      { d: 'M7 20.8l-3-2.8 3-2.8' },
      { d: 'M4 18h12.8a3.2 3.2 0 0 0 3.2-3.2v-1.6' },
    ],
    repeatOne: [
      { d: 'M17 3.2l3 2.8-3 2.8' },
      { d: 'M20 6H7.2A3.2 3.2 0 0 0 4 9.2v1.6' },
      { d: 'M7 20.8l-3-2.8 3-2.8' },
      { d: 'M4 18h12.8a3.2 3.2 0 0 0 3.2-3.2v-1.6' },
      { d: 'M11 10.6l1.6-1v4.8' },
    ],
    queue: [
      { d: 'M4 6.5h11' },
      { d: 'M4 12h11' },
      { d: 'M4 17.5h7' },
      { d: 'M17.4 13.2v5.6l3.4-2.8z', fill: true },
    ],
    volume: [
      { d: 'M4 9.4v5.2h3.1L11.4 18V6L7.1 9.4z', fill: true },
      { d: 'M14.8 9.2a3.6 3.6 0 0 1 0 5.6' },
      { d: 'M17.6 6.6a7.6 7.6 0 0 1 0 10.8' },
    ],
    mute: [
      { d: 'M4 9.4v5.2h3.1L11.4 18V6L7.1 9.4z', fill: true },
      { d: 'M15.4 9.8l4.4 4.4M19.8 9.8l-4.4 4.4' },
    ],
    close: [{ d: 'M6.4 6.4l11.2 11.2M17.6 6.4L6.4 17.6' }],
    search: [
      { d: 'M11 4.2a6.8 6.8 0 1 0 0 13.6 6.8 6.8 0 0 0 0-13.6z' },
      { d: 'M16.2 16.2L20.6 20.6' },
    ],
    folder: [
      { d: 'M3.6 7.4A2 2 0 0 1 5.6 5.4h3.2l1.8 2.2h7.8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5.6a2 2 0 0 1-2-2z' },
    ],
    clock: [
      { d: 'M12 4.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2z' },
      { d: 'M12 8.2v4.2l2.8 1.6' },
    ],
    flame: [
      {
        d: 'M12 3.6c3 3.1 5.4 5.7 5.4 8.7a5.4 5.4 0 0 1-10.8 0c0-1.7.8-3.1 2.1-4.6.5 1 1.3 1.7 2.2 2 .3-2.2.3-4.1 1.1-6.1z',
      },
    ],
    list: [{ d: 'M4 7h16' }, { d: 'M4 12h16' }, { d: 'M4 17h10' }],
    settings: [
      { d: 'M4.6 8h14.8' },
      { d: 'M4.6 16h14.8' },
      { d: 'M9.4 8a1.9 1.9 0 1 1-3.8 0 1.9 1.9 0 0 1 3.8 0z' },
      { d: 'M18.4 16a1.9 1.9 0 1 1-3.8 0 1.9 1.9 0 0 1 3.8 0z' },
    ],
    plus: [{ d: 'M12 5.5v13' }, { d: 'M5.5 12h13' }],
    grip: [
      { d: 'M9 7h1.8v1.8H9z', fill: true },
      { d: 'M9 11.1h1.8v1.8H9z', fill: true },
      { d: 'M9 15.2h1.8v1.8H9z', fill: true },
      { d: 'M13.2 7H15v1.8h-1.8z', fill: true },
      { d: 'M13.2 11.1H15v1.8h-1.8z', fill: true },
      { d: 'M13.2 15.2H15v1.8h-1.8z', fill: true },
    ],
    lyrics: [
      { d: 'M4.4 6.6h15.2v9.2h-7.1L8.2 19v-3.2H4.4z' },
      { d: 'M8 9.8h8' },
      { d: 'M8 12.6h5' },
    ],
    check: [{ d: 'M5.4 12.6l4.2 4.2 9-9' }],
    trash: [
      { d: 'M5 7h14' },
      { d: 'M9.4 7V4.8h5.2V7' },
      { d: 'M7 7l.9 12.2h8.2L17 7' },
    ],
    edit: [{ d: 'M5 19h3.2l9.2-9.2-3.2-3.2L5 15.8z' }, { d: 'M14.6 5.8l3.2 3.2' }],
    more: [
      { d: 'M6.4 12a1.4 1.4 0 1 1-2.8 0 1.4 1.4 0 0 1 2.8 0z', fill: true },
      { d: 'M13.4 12a1.4 1.4 0 1 1-2.8 0 1.4 1.4 0 0 1 2.8 0z', fill: true },
      { d: 'M20.4 12a1.4 1.4 0 1 1-2.8 0 1.4 1.4 0 0 1 2.8 0z', fill: true },
    ],
    refresh: [{ d: 'M19.6 12a7.6 7.6 0 1 1-2.3-5.4' }, { d: 'M19.6 4.4V10h-5.5' }],
    sun: [
      { d: 'M12 8.2a3.8 3.8 0 1 0 0 7.6 3.8 3.8 0 0 0 0-7.6z' },
      { d: 'M12 3.2v2M12 18.8v2M3.2 12h2M18.8 12h2M5.8 5.8l1.4 1.4M16.8 16.8l1.4 1.4M18.2 5.8l-1.4 1.4M7.2 16.8l-1.4 1.4' },
    ],
    moon: [{ d: 'M19 14.4A7.8 7.8 0 0 1 9.6 5a7.8 7.8 0 1 0 9.4 9.4z' }],
    spectrum: [
      { d: 'M4 18v-6', fill: false },
      { d: 'M8.5 18V8' },
      { d: 'M13 18v-8.5' },
      { d: 'M17.5 18V5.5' },
    ],
  };

  interface Props {
    name: IconName;
    size?: number;
    strokeWidth?: number;
  }

  let { name, size = 16, strokeWidth = 1.7 }: Props = $props();

  const shapes = $derived(ICONS[name] ?? []);
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#each shapes as shape, index (index)}
    <path d={shape.d} fill={shape.fill ? 'currentColor' : 'none'} stroke={shape.fill ? 'none' : 'currentColor'} />
  {/each}
</svg>
