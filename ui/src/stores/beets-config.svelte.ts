import type { BeetsConfig } from '../lib/ipc/generated/BeetsConfig';
import { errorMessage } from '../lib/errors';
import { ipc } from '../lib/ipc';

/**
 * beets 配置 store（docs/40 十三轮）：待整理目录 + 整理输出根。
 *
 * 两个消费方：ImportPanel 的「待整理目录」行（inbox 的展示/更换/打开/整理
 * 入库），与曲库目录列表的「整理输出」radio（单选，唯一生效）。数据来自
 * `beets_config_get`，变更走 set 命令、以**后端返回为准**回填——校验
 * （inbox 不与曲库目录重叠、target 必须在曲库目录里）在后端，这里只反映。
 *
 * 为什么独立成 store 而不塞进 ui store：消费方两处、更新路径三条（load/
 * setInbox/setTarget），放 ui store 会让那份大快照再多一个弱关联字段。
 */
class BeetsConfigStore {
  inbox = $state<string | null>(null);
  targetRoot = $state<string | null>(null);
  effectiveTarget = $state<string | null>(null);
  importedCount = $state(0);
  loaded = $state(false);

  private apply(c: BeetsConfig): void {
    this.inbox = c.inbox;
    this.targetRoot = c.targetRoot;
    this.effectiveTarget = c.effectiveTarget;
    this.importedCount = c.importedCount;
    this.loaded = true;
  }

  /** 拉一次配置（两个消费方挂载时各调一次；幂等） */
  async load(): Promise<void> {
    try {
      this.apply(await ipc.beets.config());
    } catch (e) {
      console.warn('beets.config failed:', errorMessage(e));
      this.loaded = true;
    }
  }

  /** 设置待整理目录（null = 清除）。失败抛出，调用方给内置报错 */
  async setInbox(path: string | null): Promise<void> {
    this.apply(await ipc.beets.setInbox(path));
  }

  /** 勾选整理输出根（单选）。失败抛出 */
  async setTarget(path: string): Promise<void> {
    this.apply(await ipc.beets.setTarget(path));
  }

  /** 在资源管理器打开待整理目录 */
  async openInbox(): Promise<void> {
    await ipc.beets.openInbox();
  }
}

export const beetsConfig = new BeetsConfigStore();
