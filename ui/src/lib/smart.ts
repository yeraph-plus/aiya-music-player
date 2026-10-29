/**
 * 动态歌单规则的前端形状（与 Rust `aiya-data/src/smart.rs` 一一对应）。
 *
 * **真源在 Rust**：解析与校验以 smart.rs 为准（未知 field/op 直接报错，
 * 不静默忽略）。这里只做 UI 编辑期的即时校验（让用户在保存前就看到
 * "数字字段不能填文本"），提交后后端还会再验一遍——两层校验不是冗余，
 * 是把错误拦在离用户最近的那一层。
 */

export type SmartField = 'genre' | 'artist' | 'album' | 'year' | 'playCount';
export type SmartOp = 'eq' | 'contains' | 'gte' | 'lte';
export type SmartSortKey = 'title' | 'added' | 'artist' | 'album';

export interface SmartCond {
  field: SmartField;
  op: SmartOp;
  /**
   * `null` = 编辑期"还没填"（数字控件清空就落 null，不是 0）。
   * 提交时空值条件整体跳过（见 OverlayHost 的 submitPlaylistEdit）。
   */
  value: string | number | null;
}

export interface SmartRule {
  all: SmartCond[];
  /**
   * 排序键/方向：2026-10-28 拍板后**编辑器不再暴露**——排序由歌单视图的
   * 列头处理，保存时恒写 title/asc。字段保留是协议兼容（旧 rule_json 里
   * 有值，Rust 侧仍解析），不是可配置项。
   */
  sort?: SmartSortKey;
  direction?: 'asc' | 'desc';
  limit?: number | null;
}

/** 字段下拉的标签与语义说明（说明给 placeholder/提示行用） */
export const FIELD_LABELS: Record<SmartField, string> = {
  genre: '流派',
  artist: '艺人',
  album: '专辑',
  year: '年份',
  playCount: '播放次数',
};

/** 每个字段允许的操作符（与 Rust Field::allows 同源） */
export const OPS_BY_FIELD: Record<SmartField, { op: SmartOp; label: string }[]> = {
  genre: [
    { op: 'eq', label: '等于' },
    { op: 'contains', label: '包含' },
  ],
  artist: [
    { op: 'eq', label: '等于' },
    { op: 'contains', label: '包含' },
  ],
  album: [
    { op: 'eq', label: '等于' },
    { op: 'contains', label: '包含' },
  ],
  year: [
    { op: 'eq', label: '等于' },
    { op: 'gte', label: '不早于' },
    { op: 'lte', label: '不晚于' },
  ],
  playCount: [
    { op: 'gte', label: '至少' },
    { op: 'lte', label: '至多' },
    { op: 'eq', label: '等于' },
  ],
};

/* ---------------- 条件模式（2026-10-28 拍板：第一列选"怎么匹配"） ----------------
 *
 * 规则引擎的形状是 (field, op, value)，但那对用户是程序员词汇——编辑器把它
 * 折叠成「条件模式」：一个模式 = 固定的 field+op + 一种值控件（流派是下拉、
 * 艺人/专辑是关键词输入、年份/播放次数是数字）。已有的条件用 `modeOfCond`
 * 反推回模式，任何 field/op 组合都有归处，不会丢规则。
 */

export type CondMode =
  | 'genre'
  | 'artist'
  | 'album'
  | 'yearFrom'
  | 'yearTo'
  | 'playsMin'
  | 'playsMax';

export interface CondModeSpec {
  mode: CondMode;
  /** 第一列下拉里显示的文案（动作 + 对象） */
  label: string;
  field: SmartField;
  op: SmartOp;
  /** 值控件：流派 = 曲库流派下拉；text = 关键词；number = 数字 */
  control: 'genreSelect' | 'text' | 'number';
  placeholder: string;
}

/** 顺序即下拉顺序：标签 → 关键词 → 数字，按使用频率排 */
export const COND_MODE_ORDER: CondMode[] = [
  'genre',
  'artist',
  'album',
  'yearFrom',
  'yearTo',
  'playsMin',
  'playsMax',
];

export const COND_MODES: Record<CondMode, CondModeSpec> = {
  genre: { mode: 'genre', label: '流派', field: 'genre', op: 'eq', control: 'genreSelect', placeholder: '选择流派' },
  artist: { mode: 'artist', label: '艺人名', field: 'artist', op: 'contains', control: 'text', placeholder: '艺人名关键词' },
  album: { mode: 'album', label: '专辑名', field: 'album', op: 'contains', control: 'text', placeholder: '专辑名关键词' },
  yearFrom: { mode: 'yearFrom', label: '年份不早于', field: 'year', op: 'gte', control: 'number', placeholder: '如 1990' },
  yearTo: { mode: 'yearTo', label: '年份不晚于', field: 'year', op: 'lte', control: 'number', placeholder: '如 2020' },
  playsMin: { mode: 'playsMin', label: '播放次数至少', field: 'playCount', op: 'gte', control: 'number', placeholder: '如 5' },
  playsMax: { mode: 'playsMax', label: '播放次数至多', field: 'playCount', op: 'lte', control: 'number', placeholder: '如 20' },
};

/** 已有条件 → 归属的模式（eq 的年份/次数归"From/Min"侧，语义最接近） */
export function modeOfCond(cond: SmartCond): CondMode {
  switch (cond.field) {
    case 'genre':
      return 'genre';
    case 'artist':
      return 'artist';
    case 'album':
      return 'album';
    case 'year':
      return cond.op === 'lte' ? 'yearTo' : 'yearFrom';
    case 'playCount':
      return cond.op === 'lte' ? 'playsMax' : 'playsMin';
  }
}

/** 模式 → 空条件（值给该控件类型的"未填"态：数字 null、文本空串） */
export function condOfMode(mode: CondMode): SmartCond {
  const spec = COND_MODES[mode];
  return { field: spec.field, op: spec.op, value: spec.control === 'number' ? null : '' };
}

/** 条件是否为"空"（未填值）：提交时这类条件整体跳过 */
export function isEmptyCond(cond: SmartCond): boolean {
  return cond.value === null || (typeof cond.value === 'string' && !cond.value.trim());
}

export function isTextField(field: SmartField): boolean {
  return field === 'genre' || field === 'artist' || field === 'album';
}

/** UI 编辑期校验。返回第一条错误文案，null = 通过（后端仍会再验） */
export function validateRule(rule: SmartRule): string | null {
  for (const cond of rule.all) {
    // 空值条件提交时会被跳过，校验放行（清空数字 ≠ 填 0）
    if (isEmptyCond(cond)) continue;
    if (isTextField(cond.field)) {
      if (typeof cond.value !== 'string' || !cond.value.trim()) {
        return `${FIELD_LABELS[cond.field]}条件需要一个非空文本值`;
      }
    } else {
      if (typeof cond.value !== 'number' || !Number.isFinite(cond.value)) {
        return `${FIELD_LABELS[cond.field]}条件需要一个数字值`;
      }
    }
    if (!OPS_BY_FIELD[cond.field].some((o) => o.op === cond.op)) {
      return `${FIELD_LABELS[cond.field]}不支持该操作符`;
    }
  }
  if (rule.limit != null && (!Number.isInteger(rule.limit) || rule.limit < 0)) {
    return '上限必须是非负整数';
  }
  return null;
}
