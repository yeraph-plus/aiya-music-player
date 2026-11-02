<script lang="ts">
  import { ChevronDown, RotateCcw } from '@lucide/svelte';
  import IconButton from './ui/IconButton.svelte';
  import Knob from './ui/Knob.svelte';
  import Menu from './ui/Menu.svelte';
  import Slider from './ui/Slider.svelte';
  import Switch from './ui/Switch.svelte';
  import { EQ_FREQ_LABELS, EQ_PRESETS, eq } from '../stores/eq.svelte';

  /**
   * 均衡器面板：EQ 抽屉内容，仅在 SideDrawer（380px 卡片）内使用。
   *
   * 三区结构：预设 → 频段 → 增益。增益区为三旋钮三角布局，前置增益居上，
   * 低音与高音分居下两角：引擎中三者分别是独立的 90Hz 低架、7.5kHz 高架与
   * 抗削顶 pregain，均为改变整体听感的宏观量，集中放置比分散更易
   * 理解。旋钮交互为按住横向拖动。380px 宽度内可容纳全部 10 段，常规窗口
   * 无需滚动。
   *
   * 数据全部来自 `stores/eq.svelte.ts`（单一数据源），本组件只负责呈现与手势。
   * 预设下拉复用 kit 的 `Menu`（0×0 锚点 + 点击坐标），当前命中的预设标记 ✓。
   */
  $effect(() => {
    void eq.load();
  });

  let presetMenu = $state<{ x: number; y: number } | null>(null);

  const presetItems = $derived(
    Object.entries(EQ_PRESETS).map(([key, preset]) => ({
      label: preset.label,
      // 选中标记走 MenuItem.selected（组件画勾）——往 label 里拼 '✓ ' 会被读屏念出来
      selected: eq.presetKey === key,
      action: () => eq.applyPreset(key),
    })),
  );

  function fmtGain(g: number): string {
    const rounded = Math.round(g * 10) / 10;
    if (rounded === 0) return '0';
    return rounded > 0 ? `+${rounded}` : `${rounded}`;
  }

  /** 增益数值三态染色：+ 红 / − 绿 / 0 中性（涨红跌绿的市场惯例） */
  function valClass(g: number): string {
    return g > 0 ? 'gain-pos' : g < 0 ? 'gain-neg' : '';
  }
</script>

<section class="eq-panel" aria-label="均衡器">
  <!-- 抽屉头部：标题 + 开关 -->
  <header>
    <strong>均衡器</strong>
    <span class="dim">{eq.enabled ? '已开启' : '已关闭'}</span>
    <Switch checked={eq.enabled} onchange={(v) => eq.toggle(v)} label="启用均衡器" />
  </header>

  <div class="eq-body">
    <!-- 小标题分区（替代内嵌卡片）：与搜索页「曲目」h2 同款档位 -->
    <section class="eq-section">
      <div class="sec-head">
        <h2>预设</h2>
        <IconButton label="重置曲线" size="sm" onclick={() => eq.reset()}>
          <RotateCcw size={13} />
        </IconButton>
      </div>
      <button
        class="preset-btn"
        onclick={(event) => (presetMenu = { x: event.clientX, y: event.clientY })}
      >
        <span>{eq.presetKey ? EQ_PRESETS[eq.presetKey].label : '自定义'}</span>
        <ChevronDown size={13} />
      </button>
    </section>

    {#if presetMenu}
      <Menu x={presetMenu.x} y={presetMenu.y} items={presetItems} onclose={() => (presetMenu = null)} />
    {/if}

    <!-- 10 段横排，居中留呼吸；宽度预算 10×28 + 9×6 = 334 ≤ 354（内容宽） -->
    <section class="eq-section">
      <h2>频段</h2>
      <div class="eq-bands">
        {#each eq.bands as gain, i (i)}
          <div class="eq-band">
            <span class="eq-val">{fmtGain(gain)}</span>
            <div class="eq-track">
              <Slider
                orientation="vertical"
                value={gain}
                min={-12}
                max={12}
                step={0.5}
                disabled={!eq.enabled}
                label={`频段 ${EQ_FREQ_LABELS[i]} Hz 增益`}
                oninput={(v) => eq.setBand(i, v)}
              />
            </div>
            <span class="eq-hz">{EQ_FREQ_LABELS[i]}</span>
          </div>
        {/each}
      </div>
    </section>

    <!-- 增益：三旋钮三角布局。前置增益居上，低音与高音分居下两角，按住横向
         拖动。三者均为宏观量，集中为一块以贴近实体音响的操作形态 -->
    <section class="eq-section">
      <h2>增益</h2>
      <div class="knob-tri">
        <div class="knob-unit pos-top">
          <Knob
            value={eq.pregain}
            min={-12}
            max={12}
            step={0.5}
            disabled={!eq.enabled}
            label="前置增益"
            oninput={(v) => eq.setPregain(v)}
          />
          <span class="eq-val knob-val {valClass(eq.pregain)}">{fmtGain(eq.pregain)} dB</span>
          <span class="knob-name">前置</span>
        </div>
        <div class="knob-unit pos-left">
          <Knob
            value={eq.bass}
            min={-12}
            max={12}
            step={0.5}
            disabled={!eq.enabled}
            label="低音增益"
            oninput={(v) => eq.setBass(v)}
          />
          <span class="eq-val knob-val {valClass(eq.bass)}">{fmtGain(eq.bass)} dB</span>
          <span class="knob-name">低音</span>
        </div>
        <div class="knob-unit pos-right">
          <Knob
            value={eq.treble}
            min={-12}
            max={12}
            step={0.5}
            disabled={!eq.enabled}
            label="高音增益"
            oninput={(v) => eq.setTreble(v)}
          />
          <span class="eq-val knob-val {valClass(eq.treble)}">{fmtGain(eq.treble)} dB</span>
          <span class="knob-name">高音</span>
        </div>
      </div>
    </section>
  </div>
</section>

<style>
  /* 面板 = 头部 + 分区流；窗口过矮时面板自身才滚（常规高度全可见） */
  .eq-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--line);
    font-size: var(--fs-base);
  }

  header .dim {
    margin-right: auto;
    font-size: var(--fs-sm);
  }

  .eq-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 12px 16px 10px;
  }

  /* 分区小标题：不设外框与底色（避免卡片嵌套），标题自身承担分区线与纵向间距 */
  .eq-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .sec-head,
  .eq-section h2 {
    margin: 0;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text-2);
    letter-spacing: 0.02em;
  }

  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  /* 增益三角：前置占上中，低音/高音占下两角（3 列网格的三个角位）。
     行高让下一行旋钮与上一行有呼吸；窄格不参与（中下空着） */
  .knob-tri {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    justify-items: center;
    align-items: start;
    row-gap: 4px;
    padding: 2px 0 0;
  }

  .pos-top {
    grid-column: 2;
    grid-row: 1;
  }

  .pos-left {
    grid-column: 1;
    grid-row: 2;
  }

  .pos-right {
    grid-column: 3;
    grid-row: 2;
  }

  /* 单个旋钮位：旋钮 + dB 值（在上，主指示）+ 名称（在下），纵排居中 */
  .knob-unit {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .knob-name {
    font-size: var(--fs-sm);
    color: var(--text-1);
  }

  /* 增益数值：旋钮正下方的档位主指示，字号提到 12px；三态染色
     （+ 红 / − 绿 / 0 沿用中性灰——中国市场涨红跌绿惯例） */
  .knob-val {
    font-size: var(--fs-base);
  }

  .knob-val.gain-pos {
    color: var(--danger);
  }

  .knob-val.gain-neg {
    color: var(--ok);
  }

  /* 预设按钮：select 观感（描边 + 下箭头），点击弹 Menu；整行宽 */
  .preset-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 7px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-1);
    font-size: var(--fs-base);
    color: var(--text-1);
    text-align: left;
  }

  .preset-btn:hover {
    border-color: var(--line-strong);
    background: var(--surface-hover);
  }

  .preset-btn :global(svg) {
    color: var(--text-3);
  }

  /* 10 段横排，居中；列内容不会再横向溢出
     （竖向滑杆的 min-width:40 已在 Slider.svelte 的 .vert 档归零） */
  .eq-bands {
    display: flex;
    justify-content: center;
    gap: 6px;
  }

  .eq-band {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 28px;
  }

  /* 滑杆轨道定高：整面板 ≈ 380px，抽屉常规高度（≥420px）全可见 */
  .eq-track {
    height: 130px;
    display: flex;
    justify-content: center;
  }

  .eq-val {
    font-size: var(--fs-xs);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .eq-hz {
    font-size: var(--fs-xs);
    color: var(--text-3);
  }
</style>
