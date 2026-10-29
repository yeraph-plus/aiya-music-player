import { AUDIO_RATE_STEP } from './audio-format';
import { resolveShortcutBindings, type ShortcutAction } from './shortcuts';
import { capture } from '../stores/capture.svelte';
import { list } from '../stores/list.svelte';
import { nav } from '../stores/nav.svelte';
import { overlay } from '../stores/overlay.svelte';
import { player } from '../stores/player.svelte';
import { selection } from '../stores/selection.svelte';
import { lyrics } from '../stores/lyrics.svelte';
import { ui } from '../stores/ui.svelte';
import { focusSearch } from './focus-search';
import { listKeyIntent } from './list-keys';

/** 方向键左右一次跳多少毫秒；音量一次调多少（与播放条按钮同一档） */
const SEEK_STEP_MS = 5000;
const VOLUME_STEP = 0.05;

/**
 * 全局键盘处理。
 *
 * 从 `App.svelte` 搬出来（批次 3）的理由：这段逻辑**一个视图都不碰**——
 * 它只读 store（ui / player / list / selection / overlay）和快捷键绑定。
 * 留在 App 里就得把 5 个 store 的成员在 App 顶部各起一个 `$derived` 别名。
 *
 * 分两层，别合并：
 * 1. `listKeyIntent`（`lib/list-keys.ts`）：把「按键 + 当前界面状态」判成一个
 *    意图（移动 / 播放 / 清空选择 / 关面板 / 触发动作）。纯函数，可测试。
 * 2. 本文件：执行意图。**副作用全在这里**，所以判断错了改一处即可。
 */
/**
 * 焦点落在这些控件上时，单键快捷键必须让位：Space/Enter 是激活键，
 * 方向键是控件自己的导航。此前只排除 INPUT/TEXTAREA，Tab 到按钮上按
 * 空格会"按钮不响应 + 播放状态乱切"，聚焦列表行按空格会先起播再暂停。
 */
const INTERACTIVE_SELECTOR =
  'button, select, a, [role="slider"], [role="radio"], [role="option"], [role="menuitem"], [role="tab"]';

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable) return true;
  return el.closest(INTERACTIVE_SELECTOR) !== null;
}

/** 真正的文本输入位（isTyping 的子集）：Esc 退出输入才需要 blur */
function isTextEntry(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable);
}

function currentTrackId(): string | null {
  return player.state?.current?.id ?? null;
}

/**
 * 执行一个快捷键动作。
 *
 * 全部是**单键操作**，所以不许抛：播放条按一下就冒出红色错误条是噪音，
 * 失败只 `console.warn`（`player` store 内部已把单键失败降级）。
 */
export function runShortcut(action: ShortcutAction): void {
  const state = player.state;
  switch (action) {
    case 'playPause':
      player.toggle();
      return;
    case 'next':
      player.next();
      return;
    case 'prev':
      player.prev();
      return;
    case 'seekForward':
      player.seek((state?.positionMs ?? 0) + SEEK_STEP_MS);
      return;
    case 'seekBackward':
      player.seek((state?.positionMs ?? 0) - SEEK_STEP_MS);
      return;
    case 'volumeUp':
      player.setVolume((state?.volume ?? 0.85) + VOLUME_STEP);
      return;
    case 'volumeDown':
      player.setVolume((state?.volume ?? 0.85) - VOLUME_STEP);
      return;
    case 'rateUp':
      player.setRate((state?.rate ?? 1) + AUDIO_RATE_STEP);
      return;
    case 'rateDown':
      player.setRate((state?.rate ?? 1) - AUDIO_RATE_STEP);
      return;
    case 'mute':
      player.toggleMute();
      return;
    case 'focusSearch':
      focusSearch();
      return;
    case 'toggleQueue':
      ui.toggleQueue();
      return;
    case 'toggleLyrics':
      nav.go({ kind: 'page', page: 'nowPlaying' });
      if (currentTrackId()) lyrics.load(currentTrackId()!);
      return;
    case 'toggleShuffle':
      player.setShuffle(!(state?.shuffle ?? false));
      return;
    case 'cycleRepeat':
      // 动作 id 是持久化键不改（老配置不失效）；实现已升级为模式单键循环
      player.cyclePlayMode();
      return;
    case 'selectAll':
      selection.selectAll(list.total);
      return;
    case 'openSettings':
      nav.go({ kind: 'page', page: 'settings' });
      return;
  }
}

export function onKeydown(event: KeyboardEvent): void {
  // 行组件/歌词行/Knob 等已经 preventDefault 的按键不再重复处理：
  // 它们各自消化掉了（起播/seek/调值），全局再走一遍就是双触发
  // （聚焦列表行按空格"先起播再暂停"、设置页旋钮方向键同时 seek）。
  if (event.defaultPrevented) return;

  // 正在设置页录制快捷键：**所有**全局快捷键让路。
  // 两个 listener 都在 window 上，录制侧 stopPropagation 拦不住这一条，
  // 只能靠共享标志判断（见 stores/capture.svelte.ts）。
  if (capture.active) return;

  if (isTyping(event.target)) {
    // 文本输入框里只接管 Esc（退出输入）；按钮/滑杆等控件上的按键
    // 原样留给控件自己（Space 激活、方向步进），这里不碰焦点
    if (event.key === 'Escape' && isTextEntry(event.target)) {
      (event.target as HTMLElement).blur();
    }
    return;
  }

  const intent = listKeyIntent(event, {
    total: list.total,
    selectedIndex: selection.selectedIndex,
    hasSelection: selection.size > 0,
    hasPanel: ui.queueOpen,
    // 有模态时吞掉全局快捷键：否则在「确认删除」弹窗上敲空格会顺手播放
    modalOpen: overlay.hasModal,
    bindings: resolveShortcutBindings(ui.settings.shortcuts),
  });
  if (!intent) return;

  switch (intent.kind) {
    case 'move':
      event.preventDefault();
      selection.selectedIndex = intent.index;
      return;
    case 'play':
      event.preventDefault();
      void list.playIndex(intent.index);
      return;
    case 'clearSelection':
      // 队列/EQ 抽屉开着时 Esc 只归抽屉（SideDrawer 自己关）：两边同键
      // 同帧执行会"清选择 + 关抽屉"一起发生，前者是意外副作用
      if (ui.queueOpen || ui.eqOpen) return;
      selection.clear();
      return;
    case 'closePanel':
      ui.closeQueue();
      return;
    case 'action':
      event.preventDefault();
      runShortcut(intent.action);
      return;
  }
}
