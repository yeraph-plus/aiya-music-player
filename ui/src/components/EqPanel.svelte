<script lang="ts">
  import { RotateCcw } from '@lucide/svelte';
  import IconButton from './ui/IconButton.svelte';
  import Menu from './ui/Menu.svelte';
  import Slider from './ui/Slider.svelte';
  import Switch from './ui/Switch.svelte';
  import { EQ_FREQ_LABELS, EQ_PRESETS, eq } from '../stores/eq.svelte';

  /**
   * 均衡器面板（B5a，docs/24）：开关 + 预设 + 10 段竖向滑杆 + 前置增益。
   *
   * 数据全部来自 `stores/eq.svelte.ts`（单真相），本组件只做呈现与手势。
   * 预设下拉复用 kit `Menu`（0×0 锚点 + 点击坐标），当前命中的预设打 ✓。
   */
  $effect(() => {
    void eq.load();
  });

  let presetMenu = $state<{ x: number; y: number } | null>(null);

  const presetItems = $derived(
    Object.entries(EQ_PRESETS).map(([key, preset]) => ({
      label: (eq.presetKey === key ? '✓ ' : '') + preset.label,
      action: () => eq.applyPreset(key),
    })),
  );

  function fmtGain(g: number): string {
    const rounded = Math.round(g * 10) / 10;
    if (rounded === 0) return '0';
    return rounded > 0 ? `+${rounded}` : `${rounded}`;
  }
</script>

<section class="eq-panel" aria-label="均衡器">
  <header>
    <strong>均衡器</strong>
    <span class="dim">{eq.enabled ? '已开启' : '已关闭'}</span>
    <button class="link" onclick={(event) => (presetMenu = { x: event.clientX, y: event.clientY })}>
      预设{eq.presetKey ? `：${EQ_PRESETS[eq.presetKey].label}` : '：自定义'}
    </button>
    <IconButton label="重置曲线" size="sm" onclick={() => eq.reset()}>
      <RotateCcw size={13} />
    </IconButton>
    <Switch checked={eq.enabled} onchange={(v) => eq.toggle(v)} label="启用均衡器" />
  </header>

  {#if presetMenu}
    <Menu x={presetMenu.x} y={presetMenu.y} items={presetItems} onclose={() => (presetMenu = null)} />
  {/if}

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

  <footer>
    <span class="dim">前置增益</span>
    <Slider
      value={eq.pregain}
      min={-12}
      max={12}
      step={0.5}
      label="前置增益"
      oninput={(v) => eq.setPregain(v)}
    />
    <span class="eq-val">{fmtGain(eq.pregain)} dB</span>
  </footer>
</section>

<style>
  .eq-panel {
    display: grid;
    grid-template-rows: auto 1fr auto;
    min-height: 0;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 16px 10px;
    border-bottom: 1px solid var(--line);
    font-size: 12.5px;
  }

  header .dim {
    margin-right: auto;
    font-size: 11px;
  }

  /* 10 段横排；段内 = 数值 / 竖向滑杆 / 频率标签 三行 */
  .eq-bands {
    display: flex;
    justify-content: center;
    gap: 14px;
    padding: 18px 16px 8px;
    min-height: 0;
    overflow-y: auto;
  }

  .eq-band {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 34px;
  }

  .eq-track {
    flex: 1;
    min-height: 120px;
    display: flex;
    justify-content: center;
  }

  .eq-val {
    font-size: 10.5px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .eq-hz {
    font-size: 10.5px;
    color: var(--text-3);
  }

  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px 14px;
    border-top: 1px solid var(--line);
    font-size: 12px;
  }

  footer .eq-val {
    min-width: 52px;
    text-align: right;
  }
</style>
