import type { BeetsFieldCatalog, BeetsQueryHit } from '../lib/ipc';
import { ipc } from '../lib/ipc';
import { errorMessage } from '../lib/errors';
import { library } from './library.svelte';
import { notify } from './notify.svelte';

/**
 * beets 查询页的状态与动作（docs/48）。
 *
 * ## 这一层为什么不放在组件里
 *
 * 页面要串起四件事：查字段目录 → 组装查询 → 按行解析"这在曲库里是哪首"
 * → 存成歌单。中间任何一步失败都要能局部报错（目录失败不该清掉已有结果，
 * 保存失败不该丢掉选择）。这些都写进组件的 `$state` 里，组件会变成一坨
 * 互相纠缠的 if；放在这里，组件只负责画。
 *
 * ## 为什么结果行要带 `trackId`
 *
 * beets 库里的一条命中只是"另一个库有这么个文件"，跟本应用曲库没有任何
 * 身份关系——DTO 里刻意没有 `track_id`（见 `dto::beets::BeetsQueryHit`）。
 * 但"存为歌单"只能装本库的曲目。所以查完立刻问宿主一次
 * `library.resolvePaths`，把每行标注成"已入库 / 未入库"：
 * - 未入库的行**标灰、不可选**（不造行——那会让歌单点开就是"文件缺失"）；
 * - 用户点"存为歌单"之前就知道会少几首，而不是存完才发现。
 */

/** 一条结构化查询规则（字段/算子/值，与工具侧 `QueryRule` 同名同形） */
export interface QueryRule {
  field: string;
  /** 空字符串 = 还没选（提交前会被拦下） */
  op: string;
  value: string;
}

export type QueryMode = 'rules' | 'raw';

/** 结果表的一行：命中 + 它在本曲库里的身份（`null` = 未入库） */
export interface ResultRow {
  hit: BeetsQueryHit;
  trackId: string | null;
}

/** 回带字段的基线：任何查询都要能显示"这是哪首歌" */
const BASE_FIELDS = ['title', 'artist', 'album'] as const;

/**
 * 按字段类型给可用算子。
 *
 * 只暴露**这里能正确填值**的那几个：`range` 要两个数字、`in` 要数组，
 * 两者的输入控件都不是一个输入框，硬塞进单行会得到一个"看着能填、填了
 * 不生效"的界面。需要它们时走 `raw`（beets 自己的语法本来就是为此存在的）。
 */
export function opsFor(kind: string): string[] {
  if (kind === 'integer' || kind === 'float') return ['gte', 'lte', 'eq'];
  if (kind === 'bool') return ['eq'];
  // string / date / duration / flexible：都是文本比较，语义由工具侧定义
  return ['contains', 'eq', 'prefix', 'regex'];
}

/** 算子 → 人话（下拉里显示这个，不是英文缩写） */
export const OP_LABELS: Record<string, string> = {
  contains: '包含',
  eq: '等于',
  prefix: '开头是',
  regex: '匹配正则',
  gte: '≥',
  lte: '≤',
};

/** 数字类字段：值要按数字发过去，否则工具侧会做字符串比较 */
function isNumeric(kind: string): boolean {
  return kind === 'integer' || kind === 'float';
}

class BeetsQueryStore {
  /* ---------------- 字段目录 ---------------- */

  catalog = $state<BeetsFieldCatalog | null>(null);
  catalogError = $state<string | null>(null);
  catalogLoading = $state(false);

  /** 当前可选的字段（分类给下拉用）。目录没拿到时只给基线三个。 */
  get fields(): { name: string; kind: string }[] {
    const list = this.catalog?.fields ?? [];
    if (list.length === 0) return BASE_FIELDS.map((name) => ({ name, kind: 'string' }));
    return list.map((f) => ({ name: f.name, kind: f.type }));
  }

  /** 识别源三态（probe / xtractor / lastgenre …），页面上显示成一行小字 */
  get sources(): { name: string; state: string }[] {
    const raw = this.catalog?.sources;
    if (!Array.isArray(raw)) return [];
    return raw.filter(
      (s): s is { name: string; state: string } =>
        typeof s === 'object' && s !== null && 'name' in s && 'state' in s,
    );
  }

  async loadCatalog(): Promise<void> {
    if (this.catalogLoading) return;
    this.catalogLoading = true;
    try {
      this.catalog = await ipc.beets.fields();
      this.catalogError = null;
    } catch (e) {
      // 目录失败**不清结果**：用户可能已经查到一页，凭什么因为下拉框坏了就清空
      this.catalogError = errorMessage(e);
    } finally {
      this.catalogLoading = false;
    }
  }

  /* ---------------- 查询条件 ---------------- */

  mode = $state<QueryMode>('rules');
  rules = $state<QueryRule[]>([]);
  raw = $state('');
  limit = $state(200);

  addRule(): void {
    const first = this.fields[0];
    this.rules = [
      ...this.rules,
      { field: first?.name ?? 'title', op: opsFor(first?.kind ?? 'string')[0], value: '' },
    ];
  }

  removeRule(index: number): void {
    this.rules = this.rules.filter((_, i) => i !== index);
  }

  /** 换字段时算子要跟着收窄：`bpm` 上没有"包含"这种说法 */
  setRuleField(index: number, field: string): void {
    const kind = this.fields.find((f) => f.name === field)?.kind ?? 'string';
    const allowed = opsFor(kind);
    this.rules = this.rules.map((r, i) =>
      i === index
        ? { ...r, field, op: allowed.includes(r.op) ? r.op : allowed[0] }
        : r,
    );
  }

  setRuleOp(index: number, op: string): void {
    this.rules = this.rules.map((r, i) => (i === index ? { ...r, op } : r));
  }

  setRuleValue(index: number, value: string): void {
    this.rules = this.rules.map((r, i) => (i === index ? { ...r, value } : r));
  }

  /**
   * 回带字段 = 基线三列 + **当前条件里用到的非基线字段**。
   *
   * 为什么要跟条件走：按 `bpm >= 120` 筛出十首，表里却没有 bpm 列，用户
   * 无从判断筛选是否按他想的生效。`raw` 模式下语法由 beets 解释、这里不
   * 做解析（解析一份别人的 DSL 就是第二份实现），所以只回基线三列。
   */
  get returnFields(): string[] {
    const extra =
      this.mode === 'rules'
        ? this.rules
            .map((r) => r.field)
            .filter((f) => f && !(BASE_FIELDS as readonly string[]).includes(f))
        : [];
    return [...BASE_FIELDS, ...Array.from(new Set(extra))];
  }

  /* ---------------- 结果 ---------------- */

  rows = $state<ResultRow[]>([]);
  total = $state(0);
  truncated = $state(false);
  /** 查过一次才算"查过"——否则空结果说不清是"没查"还是"没命中" */
  ran = $state(false);
  running = $state(false);
  error = $state<string | null>(null);

  get hitCount(): number {
    return this.rows.length;
  }

  get missingCount(): number {
    return this.rows.filter((r) => r.trackId === null).length;
  }

  private buildFilter(): { raw?: string; rules?: QueryRule[]; limit: number } | string {
    if (this.mode === 'raw') {
      const text = this.raw.trim();
      if (!text) return '原始语法是空的——要么写一句查询，要么切回「条件」';
      return { raw: text, limit: this.limit };
    }
    if (this.rules.length === 0) {
      // 无条件 = 全库，这是合法查询（"看看我这库里都有什么"）
      return { limit: this.limit };
    }
    const out: { field: string; op: string; value: unknown }[] = [];
    for (const [i, rule] of this.rules.entries()) {
      const kind = this.fields.find((f) => f.name === rule.field)?.kind ?? 'string';
      const text = rule.value.trim();
      if (!text) return `第 ${i + 1} 条条件还没有填值`;
      if (isNumeric(kind)) {
        const n = Number(text);
        if (!Number.isFinite(n)) return `第 ${i + 1} 条条件的值要填数字（${rule.field}）`;
        out.push({ field: rule.field, op: rule.op, value: n });
      } else if (kind === 'bool') {
        const lowered = text.toLowerCase();
        if (lowered !== 'true' && lowered !== 'false') {
          return `第 ${i + 1} 条条件的值只能是 true / false`;
        }
        out.push({ field: rule.field, op: rule.op, value: lowered === 'true' });
      } else {
        out.push({ field: rule.field, op: rule.op, value: text });
      }
    }
    return { rules: out as QueryRule[], limit: this.limit };
  }

  async run(): Promise<void> {
    if (this.running) return;
    const filter = this.buildFilter();
    if (typeof filter === 'string') {
      this.error = filter;
      return;
    }
    this.running = true;
    this.error = null;
    this.selected = new Set();
    try {
      const page = await ipc.beets.query({ ...filter, fields: this.returnFields });
      // 一次往返把"这些命中在本曲库里是哪首"问清楚（同序等长）
      const ids = page.items.length
        ? await ipc.library.resolvePaths(page.items.map((h) => h.path))
        : [];
      this.rows = page.items.map((hit, i) => ({ hit, trackId: ids[i] ?? null }));
      this.total = page.total;
      this.truncated = page.truncated;
      this.ran = true;
    } catch (e) {
      // 失败保留上一次结果：查询写错时清空列表等于把用户的上一次成果也收了
      this.error = errorMessage(e);
    } finally {
      this.running = false;
    }
  }

  /* ---------------- 选择 ---------------- */

  /** 选中的 `path` 集合。用 path 当键：`hit.id` 只在同一个 beets 库里成立 */
  selected = $state<Set<string>>(new Set());

  /** 只有已入库的行能被选中——未入库的选不了，也就不会"存了却没进去" */
  get selectablePaths(): string[] {
    return this.rows.filter((r) => r.trackId !== null).map((r) => r.hit.path);
  }

  get selectedTrackIds(): string[] {
    const ids = this.rows
      .filter((r) => r.trackId !== null && this.selected.has(r.hit.path))
      .map((r) => r.trackId as string);
    // 同一首曲目可能出现在两行（同一文件被 beets 收了两次），去重保序
    return Array.from(new Set(ids));
  }

  isSelected(path: string): boolean {
    return this.selected.has(path);
  }

  toggle(path: string): void {
    const next = new Set(this.selected);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    this.selected = next;
  }

  /** 全选 = 选中当前页**已入库**的全部（不跨页：查询结果本来就只到 limit） */
  get allSelected(): boolean {
    const paths = this.selectablePaths;
    return paths.length > 0 && paths.every((p) => this.selected.has(p));
  }

  toggleAll(): void {
    this.selected = this.allSelected ? new Set() : new Set(this.selectablePaths);
  }

  clearSelection(): void {
    this.selected = new Set();
  }

  /* ---------------- 存为歌单 ---------------- */

  saveOpen = $state(false);
  saveName = $state('');
  saving = $state(false);
  saveError = $state<string | null>(null);

  openSave(): void {
    if (this.selectedTrackIds.length === 0) {
      notify.showToast('先选中几首已入库的曲目');
      return;
    }
    // 预填一个能直接用的名字：beets 查询页存出来的歌单多半是"某次筛选的结果"
    if (!this.saveName.trim()) this.saveName = this.suggestName();
    this.saveError = null;
    this.saveOpen = true;
  }

  closeSave(): void {
    this.saveOpen = false;
    this.saveError = null;
  }

  /** 建议名：优先用条件里第一个有值的字段，否则叫"Beets 查询" */
  private suggestName(): string {
    if (this.mode === 'rules') {
      const first = this.rules.find((r) => r.value.trim());
      if (first) return `${first.field} ${OP_LABELS[first.op] ?? first.op} ${first.value.trim()}`;
    }
    return 'Beets 查询';
  }

  /**
   * 存成歌单。同名歌单是**合并**（后端的 `playlist_create` 按名字派生 id，
   * 重复曲目跳过），所以界面上要写清楚这一点，否则用户会以为点了两次会
   * 得到两个歌单。
   */
  async save(): Promise<boolean> {
    if (this.saving) return false;
    const trackIds = this.selectedTrackIds;
    if (trackIds.length === 0) {
      this.saveError = '没有可保存的曲目（选中的都不在曲库里）';
      return false;
    }
    this.saving = true;
    this.saveError = null;
    try {
      const summary = await ipc.playlist.create(this.saveName.trim(), trackIds);
      // 侧栏由 `library.playlists` 驱动，创建后必须重拉——否则新歌单要等重启
      await library.loadPlaylists();
      notify.showToast(`已存为歌单「${summary.name}」（共 ${summary.trackCount} 首）`);
      this.saveOpen = false;
      this.saveName = '';
      this.clearSelection();
      return true;
    } catch (e) {
      this.saveError = errorMessage(e);
      return false;
    } finally {
      this.saving = false;
    }
  }

  /** 离开页面/换库时把结果与条件一起清掉（状态跨页残留比"少个功能"更迷） */
  reset(): void {
    this.rows = [];
    this.total = 0;
    this.truncated = false;
    this.ran = false;
    this.error = null;
    this.selected = new Set();
    this.saveOpen = false;
    this.saveName = '';
    this.saveError = null;
  }
}

export const beetsQuery = new BeetsQueryStore();
