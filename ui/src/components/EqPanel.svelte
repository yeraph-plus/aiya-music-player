<script lang="ts">
  import { RotateCcw, X } from '@lucide/svelte';
  import IconButton from './ui/IconButton.svelte';
  import Select from './ui/Select.svelte';
  import Slider from './ui/Slider.svelte';
  import Switch from './ui/Switch.svelte';
  import { t } from '../lib/i18n.svelte';
  import { EQ_FREQ_LABELS, EQ_PRESETS, eq } from '../stores/eq.svelte';

  let { onclose }: { onclose: () => void } = $props();
  $effect(() => { void eq.load(); });
  const presets = $derived(Object.entries(EQ_PRESETS).map(([value, preset]) => ({ value, label: t(preset.label) })));
  const gains = $derived([
    { label: t('eq.pregain'), value: eq.pregain, change: (v: number) => eq.setPregain(v) },
    { label: t('eq.bassGain'), value: eq.bass, change: (v: number) => eq.setBass(v) },
    { label: t('eq.trebleGain'), value: eq.treble, change: (v: number) => eq.setTreble(v) },
  ]);
  function fmtGain(g: number): string {
    const rounded = Math.round(g * 10) / 10;
    return rounded > 0 ? `+${rounded}` : `${rounded}`;
  }
</script>

<section class="eq-panel" aria-label={t('eq.title')}>
  <header>
    <strong>{t('eq.title')}</strong>
    <span class="dim">{eq.enabled ? t('eq.on') : t('eq.off')}</span>
    <Switch checked={eq.enabled} onchange={(v) => eq.toggle(v)} label={t('eq.enableToggle')} />
    <IconButton label={t('common.close')} size="sm" onclick={onclose}><X size={14} /></IconButton>
  </header>
  <div class="eq-body">
    <section class="eq-section">
      <div class="sec-head">
        <h2>{t('eq.presets')}</h2>
        <IconButton label={t('eq.resetCurve')} size="sm" onclick={() => eq.reset()}><RotateCcw size={14} /></IconButton>
      </div>
      <Select label={t('eq.presets')} value={eq.presetKey ?? ''} options={presets} placeholder={t('eq.custom')} onchange={(v) => eq.applyPreset(v)} />
    </section>
    <section class="eq-section">
      <h2>{t('eq.bands')}</h2>
      <div class="eq-bands">
        {#each eq.bands as gain, i (i)}
          <div class="eq-band">
            <span class="eq-val">{fmtGain(gain)}</span>
            <div class="eq-track">
              <Slider orientation="vertical" value={gain} min={-12} max={12} step={0.5}
                valueText={`${fmtGain(gain)} dB`} disabled={!eq.enabled}
                label={t('eq.bandGainAria', { freq: EQ_FREQ_LABELS[i] })} oninput={(v) => eq.setBand(i, v)} />
            </div>
            <span class="eq-hz">{EQ_FREQ_LABELS[i]}</span>
          </div>
        {/each}
      </div>
    </section>
    <section class="eq-section">
      <h2>{t('eq.gain')}</h2>
      {#each gains as gain (gain.label)}
        <div class="eq-gain">
          <div class="gain-label"><span>{gain.label}</span><output>{fmtGain(gain.value)} dB</output></div>
          <Slider value={gain.value} min={-12} max={12} step={0.5} valueText={`${fmtGain(gain.value)} dB`}
            disabled={!eq.enabled} label={gain.label} oninput={gain.change} />
        </div>
      {/each}
    </section>
  </div>
</section>

<style>
  .eq-panel { display:flex; flex-direction:column; height:100%; overflow-y:auto; }
  header { display:flex; align-items:center; gap:var(--space-2); padding:var(--space-3) var(--space-4); border-bottom:1px solid var(--line); font-size:var(--fs-md); }
  header .dim { margin-right:auto; font-size:var(--fs-sm); }
  .eq-body { display:flex; flex-direction:column; gap:var(--space-4); padding:var(--space-4); }
  .eq-section { display:flex; flex-direction:column; gap:var(--space-3); }
  .sec-head { display:flex; align-items:center; justify-content:space-between; }
  h2 { margin:0; font-size:var(--fs-sm); font-weight:600; color:var(--text-2); }
  .eq-bands { display:grid; grid-template-columns:repeat(10,minmax(0,1fr)); gap:var(--space-1); }
  .eq-band { display:flex; flex-direction:column; align-items:center; gap:var(--space-2); min-width:0; }
  .eq-track { height:8rem; display:flex; justify-content:center; }
  .eq-val, .eq-hz { font-size:var(--fs-xs); color:var(--text-2); font-variant-numeric:tabular-nums; }
  .eq-gain { display:flex; flex-direction:column; gap:var(--space-1); }
  .gain-label { display:flex; justify-content:space-between; gap:var(--space-2); font-size:var(--fs-sm); }
  output { color:var(--text-2); font-variant-numeric:tabular-nums; }
</style>
