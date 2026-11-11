/** eq 域：均衡器面板（EqPanel）与 stores/eq.svelte 的文案（预设名、失败吐司）。
 *  频段短标签（31/63/…/16k）与 `dB` 单位是语言中立记号，不进表。
 *  zh 值与界面现行中文逐字一致（e2e 断言中文文案）；en 按术语表新写
 *  （均衡器=EQ，UI 标签 sentence case；错误句小写开头）。
 *  en 表 `Record<keyof typeof zh, string>` 保证键位奇偶。 */

export const eqZh = {
  /* EqPanel：头部与开关 */
  'eq.title': '均衡器',
  'eq.on': '已开启',
  'eq.off': '已关闭',
  'eq.enableToggle': '启用均衡器',

  /* EqPanel：三个分区小标题与预设 */
  'eq.presets': '预设',
  'eq.resetCurve': '重置曲线',
  'eq.custom': '自定义',
  'eq.bands': '频段',
  'eq.bandGainAria': '频段 {freq} Hz 增益',
  'eq.gain': '增益',

  /* EqPanel：三旋钮（label + 旋钮名） */
  'eq.pregain': '前置增益',
  'eq.pregainShort': '前置',
  'eq.bassGain': '低音增益',
  'eq.bassShort': '低音',
  'eq.trebleGain': '高音增益',
  'eq.trebleShort': '高音',

  /* stores/eq.svelte.ts：预设名（EQ_PRESETS.label 存键，EqPanel 渲染时 t()） */
  'eq.preset.flat': '平直',
  'eq.preset.bass': '重低音',
  'eq.preset.vocal': '人声',
  'eq.preset.treble': '高亮',
  'eq.preset.pop': '流行',
  'eq.preset.rock': '摇滚',
  'eq.preset.jazz': '爵士',
  'eq.preset.classical': '古典',

  /* stores/eq.svelte.ts：下发失败错误条 */
  'eq.applyFailed': '均衡器设置失败：{message}',
} as const;

export const eqEn: Record<keyof typeof eqZh, string> = {
  'eq.title': 'EQ',
  'eq.on': 'On',
  'eq.off': 'Off',
  'eq.enableToggle': 'Enable EQ',

  'eq.presets': 'Presets',
  'eq.resetCurve': 'Reset curve',
  'eq.custom': 'Custom',
  'eq.bands': 'Bands',
  'eq.bandGainAria': '{freq} Hz band gain',
  'eq.gain': 'Gain',

  'eq.pregain': 'Pregain',
  'eq.pregainShort': 'Pre',
  'eq.bassGain': 'Bass gain',
  'eq.bassShort': 'Bass',
  'eq.trebleGain': 'Treble gain',
  'eq.trebleShort': 'Treble',

  'eq.preset.flat': 'Flat',
  'eq.preset.bass': 'Bass boost',
  'eq.preset.vocal': 'Vocal',
  'eq.preset.treble': 'Treble boost',
  'eq.preset.pop': 'Pop',
  'eq.preset.rock': 'Rock',
  'eq.preset.jazz': 'Jazz',
  'eq.preset.classical': 'Classical',

  'eq.applyFailed': 'failed to apply EQ settings: {message}',
};
