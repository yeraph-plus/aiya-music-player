/**
 * 拖放的业务侧：悬停蒙版的状态 + 松手后的落点处理（docs/52 §5）。
 *
 * 分两层是刻意的：
 * - `lib/dropzone.ts` 只负责"宿主怎么把文件拖进来告诉我"（纯 Tauri 绑定，
 *   预览环境是空操作）；
 * - 这里负责"拖进来的东西该怎么解释给用户"——分类、进不进队列、说什么话。
 *
 * ## 用户拍板的两条（docs/52 §10）
 *
 * 1. **文件夹不识别**：一次误拖不该触发几万次 `stat`，那是曲库导入的活。
 *    分类器（`src-tauri/src/drop.rs`）已经把它判成 `Directory` 并给出
 *    "文件夹不展开，走设置 → 曲库导入"的人话，这里只负责把它说出来。
 * 2. **所有提示走播放条上方的吐司位**（`Toast` 的 `bottom: --player-h + 16px`），
 *    不新造浮层——拖放是个"顺手的动作"，反馈不该抢走视线。
 */
import { errorMessage } from '../lib/errors';
import { ipc } from '../lib/ipc';
import type { DroppedItem } from '../lib/ipc/generated/DroppedItem';
import { onFileDrop } from '../lib/dropzone';
import { notify } from './notify.svelte';

/**
 * 拖入内容的预告（蒙版上的第二行字）。
 *
 * 为什么要在 `enter` 就分类：拖到窗口上到松手之间有一两秒，这段时间足够
 * 说清"会发生什么"。等松手再说就晚了——用户的预期已经形成了。
 */
function summarize(items: DroppedItem[]): string {
  let playable = 0;
  let playlists = 0;
  let folders = 0;
  let unsupported = 0;

  for (const item of items) {
    if (item.kind === 'audio' || item.kind === 'hlsPlaylist') playable += 1;
    else if (item.kind === 'playlist') playlists += 1;
    else if (item.kind === 'directory') folders += 1;
    else unsupported += 1;
  }

  const parts: string[] = [];
  if (playable) parts.push(`${playable} 项可播放`);
  if (playlists) parts.push(`${playlists} 个歌单会展开`);
  if (folders) parts.push(`${folders} 个文件夹跳过（走设置 → 曲库导入）`);
  if (unsupported) parts.push(`${unsupported} 项格式不支持`);
  return parts.length ? parts.join(' · ') : '没有可识别的内容';
}

/** 吐司里最多列几条原因：多到刷屏就没人看了，剩下的用「N 项」概括 */
const MAX_REASONS = 2;

function reasons(list: string[]): string {
  const head = list.slice(0, MAX_REASONS).join('；');
  return list.length > MAX_REASONS ? `${head} 等 ${list.length} 项` : head;
}

class DropStore {
  /** 有文件正悬在窗口上：AppShell 据此显示蒙版 */
  hovering = $state(false);
  /** 蒙版第二行字；分类还没回来时是 `null`（蒙版先出，字后到） */
  hint = $state<string | null>(null);

  /**
   * `enter` 的分类是异步的，`leave`/`drop` 随时可能先到。
   * 用自增 token 作废在途的那一次——否则"拖着 A 进来、快速离去、再拖 B 进来"
   * 时，A 的回复会把 B 的预告覆盖掉。
   */
  private token = 0;

  /** 订阅拖放；返回清理函数。App 的 onMount 生命周期负责调用 */
  attach(): () => void {
    return onFileDrop({
      onEnter: (paths) => void this.enter(paths),
      onLeave: () => this.clear(),
      onDrop: (paths) => void this.drop(paths),
    });
  }

  private clear(): void {
    this.token += 1;
    this.hovering = false;
    this.hint = null;
  }

  private async enter(paths: string[]): Promise<void> {
    if (!paths.length) return;
    this.hovering = true;
    const token = ++this.token;
    try {
      const items = await ipc.live.classify(paths);
      if (token !== this.token) return;
      this.hint = summarize(items);
    } catch (error) {
      // 分类只影响预告文案：后端在落点时会重新判一次，这里坏了也不影响播放
      if (token === this.token) this.hint = `${paths.length} 项`;
      console.warn('classify_paths failed:', errorMessage(error));
    }
  }

  private async drop(paths: string[]): Promise<void> {
    // 先收蒙版：落点结果由吐司接力，蒙版多留一帧会挡住正在切换的页面
    this.clear();
    if (!paths.length) return;

    try {
      const receipt = await ipc.player.openPaths(paths);

      if (receipt.opened === 0) {
        notify.showToast(
          `没有可播放的内容：${reasons(receipt.skipped)}`,
          receipt.skipped.length ? 6000 : 2600,
        );
        return;
      }

      const skipped = receipt.skipped.length
        ? `，跳过 ${reasons(receipt.skipped)}`
        : '';
      notify.showToast(`已打开 ${receipt.opened} 项${skipped}`, receipt.skipped.length ? 6000 : 2600);
    } catch (error) {
      // 走到这里是**故障**（命令本身失败），不是"拖进来的东西不合适"：
      // 保留常驻的错误条，别让一条 3 秒后消失的吐司把故障吃掉
      notify.showError(errorMessage(error));
    }
  }
}

export const drop = new DropStore();
