import type { EqSettings } from '../lib/ipc/generated';
import { ipc } from '../lib/ipc/client';
import { errorMessage } from '../lib/errors';
import { notify } from './notify.svelte';

/**
 * EQ 状态（协议 v6，docs/24 B5b/B5a）。
 *
 * 与后端的分工：
 * - **真相在引擎**：滑杆拖动 300ms 防抖后 `player.setEq`，**以返回值**（clamp 后
 *   的生效值）回写本地——用户拖到 +20 会被 clamp 到 +12，画面必须跟着回弹，
 *   不能停在自己的输入值上（那会与实际声音分叉）。
 * - 持久化在 Rust 侧独立键 `eq`（不进 `app_settings`）：EQ 高频连调与设置页
 *   低频整体写分键，互不覆盖（见 player_set_eq 的文档注释）。
 * - `load()` 在面板第一次打开时调用一次；之后全部走本地状态。
 *
 * 频段与预设只是 UI 概念：wire 上只有 10 个数字 + pregain + 开关。
 */

/** 频段短标签（与 aiya_audio::EQ_BAND_HZ 一一对应） */
export const EQ_FREQ_LABELS = ['31', '63', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'] as const;
/** 预设：10 段增益（dB）。命名取主流播放器的常见集合，数值是常识起点不是玄学 */
export const EQ_PRESETS: Record<string, { label: string; bands: number[] }> = {
  flat: { label: '平直', bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  bass: { label: '重低音', bands: [7, 6, 4, 2, 0, 0, 0, 0, 0, 0] },
  vocal: { label: '人声', bands: [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1] },
  treble: { label: '高亮', bands: [0, 0, 0, 0, 0, 1, 2, 4, 6, 7] },
  pop: { label: '流行', bands: [2, 3, 4, 2, -1, -1, 1, 3, 4, 2] },
  rock: { label: '摇滚', bands: [5, 4, 2, -1, -2, 0, 2, 4, 5, 4] },
  jazz: { label: '爵士', bands: [3, 2, 1, 2, -1, -1, 0, 1, 2, 3] },
  classical: { label: '古典', bands: [0, 0, 0, 0, 0, 0, -1, -1, 0, 2] },
};

function isPreset(bands: readonly number[]): string | null {
  for (const [key, preset] of Object.entries(EQ_PRESETS)) {
    if (preset.bands.every((g, i) => g === bands[i])) return key;
  }
  return null;
}

class EqStore {
  /** 面板数据是否已从后端回读过（只读一次，之后走本地） */
  loaded = $state(false);
  enabled = $state(false);
  bands = $state<number[]>([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  /** 低音旋钮（dB，±12）：引擎侧 90 Hz 低架，独立于图示段（docs/39） */
  bass = $state(0);
  /** 高音旋钮（dB，±12）：引擎侧 7.5 kHz 高架，独立于图示段 */
  treble = $state(0);
  pregain = $state(0);

  /** 当前是否恰好命中某个预设的曲线（是则菜单里显示它，否则"自定义"）。
   * 只看 bands：预设定义的是 10 段增益，pregain 是独立旋钮，调它不改曲线身份 */
  get presetKey(): string | null {
    return isPreset(this.bands);
  }

  private timer: ReturnType<typeof setTimeout> | undefined;

  /** 面板打开时回读一次。失败不阻塞面板（保持默认值，滑杆照样能动） */
  async load(): Promise<void> {
    if (this.loaded) return;
    try {
      const s = await ipc.player.getEq();
      this.enabled = s.enabled;
      this.bands = [...s.bands];
      this.bass = s.bassDb;
      this.treble = s.trebleDb;
      this.pregain = s.pregain;
      this.loaded = true; // 成功才置位：读失败下次打开面板会重试
    } catch {
      // 静默：EQ 是锦上添花，读不到不该弹错误条
    }
  }

  private scheduleApply(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.apply(), 300);
  }

  private async apply(): Promise<void> {
    // 快照发送时刻的值。响应**不整包回写**：往返期间（~300ms 防抖 + IPC）
    // 用户可能已经改了别的段，整包覆盖会把最新输入冲掉，滑杆闪回旧值。
    // 只回写 clamp 产生差异、且用户此后没再动过的字段。
    const sent = { bands: [...this.bands], bassDb: this.bass, trebleDb: this.treble, pregain: this.pregain };
    try {
      const applied = await ipc.player.setEq({
        enabled: this.enabled,
        bands: sent.bands,
        bassDb: sent.bassDb,
        trebleDb: sent.trebleDb,
        pregain: sent.pregain,
      });
      if (applied.bassDb !== sent.bassDb && this.bass === sent.bassDb) {
        this.bass = applied.bassDb;
      }
      if (applied.trebleDb !== sent.trebleDb && this.treble === sent.trebleDb) {
        this.treble = applied.trebleDb;
      }
      if (applied.pregain !== sent.pregain && this.pregain === sent.pregain) {
        this.pregain = applied.pregain;
      }
      for (let i = 0; i < applied.bands.length && i < sent.bands.length; i++) {
        if (applied.bands[i] !== sent.bands[i] && this.bands[i] === sent.bands[i]) {
          this.bands[i] = applied.bands[i];
        }
      }
    } catch (e) {
      // 下发失败回滚显示值（画面对齐引擎真值），并且必须让用户看见：
      // 没有全局 transport 分类层这种机制——那句话引用的是不存在的约定
      notify.showError(`均衡器设置失败：${errorMessage(e)}`);
    }
  }

  setBand(index: number, value: number): void {
    if (index < 0 || index >= this.bands.length) return;
    this.bands[index] = value;
    this.scheduleApply();
  }

  setBass(value: number): void {
    this.bass = value;
    this.scheduleApply();
  }

  setTreble(value: number): void {
    this.treble = value;
    this.scheduleApply();
  }

  setPregain(value: number): void {
    this.pregain = value;
    this.scheduleApply();
  }

  toggle(enabled: boolean): void {
    this.enabled = enabled;
    this.scheduleApply();
  }

  applyPreset(key: string): void {
    const preset = EQ_PRESETS[key];
    if (!preset) return;
    this.bands = [...preset.bands];
    this.pregain = 0;
    this.enabled = true;
    this.scheduleApply();
  }

  /** 全部归零（不关开关：用户重置的是曲线，不是意图） */
  reset(): void {
    this.bands = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    this.bass = 0;
    this.treble = 0;
    this.pregain = 0;
    this.scheduleApply();
  }
}

export const eq = new EqStore();
