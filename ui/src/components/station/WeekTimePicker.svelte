<script lang="ts">
  import { onDestroy } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { StationQueueItem, StationSlot } from '../../lib/ipc/generated';
  import { hm, minutes, validSlot, WEEK_DAYS } from '../../lib/station-time';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Field from '../ui/Field.svelte';
  import { Plus, X } from '@lucide/svelte';

  let { item, items, timezone, onchange, disabled=false }: {
    item:StationQueueItem;items:StationQueueItem[];timezone:string|null;
    onchange:(slots:StationSlot[])=>void;disabled?:boolean;
  } = $props();
  type Cell={day:number;hour:number};
  const hours=Array.from({length:24},(_,i)=>i);
  let grid:HTMLDivElement;
  let anchor=$state<Cell|null>(null);
  let endpoint=$state<Cell|null>(null);
  let focused=$state(0);
  const candidate=$derived(anchor && endpoint ? range(anchor,endpoint):null);
  function range(a:Cell,b:Cell):StationSlot {
    let mask=0;for(let day=Math.min(a.day,b.day);day<=Math.max(a.day,b.day);day++)mask|=1<<day;
    return {startHm:hm(Math.min(a.hour,b.hour)*60),endHm:hm((Math.max(a.hour,b.hour)+1)*60),daysMask:mask};
  }
  function stop():void {anchor=null;endpoint=null;document.documentElement.classList.remove('station-selecting');}
  onDestroy(stop);
  function pointerMove(event:PointerEvent):void {
    if(!anchor)return;
    const first=grid.querySelector<HTMLElement>('[data-index="0"]')!.getBoundingClientRect();
    endpoint={day:Math.max(0,Math.min(6,Math.floor((event.clientY-first.top)/first.height))),hour:Math.max(0,Math.min(23,Math.floor((event.clientX-first.left)/first.width)))};
  }
  function pointerUp(event:PointerEvent):void {
    if(!anchor)return;
    pointerMove(event);
    if(candidate && !disabled)onchange([...item.slots,candidate]);
    stop();
  }
  function timeInput(value:string):string {
    const clean=value.replace(/[^0-9:]/g,'');
    const colon=clean.indexOf(':');
    if(colon===1 || colon===2)return `${clean.slice(0,colon).padStart(2,'0')}:${clean.slice(colon+1).replace(/:/g,'').slice(0,2)}`;
    const digits=value.replace(/[^0-9]/g,'').slice(0,4);
    return digits.length>2?`${digits.slice(0,2)}:${digits.slice(2)}`:digits;
  }
  function add():void {onchange([...item.slots,{startHm:'08:00',endHm:'10:00',daysMask:1}]);}
  function update(index:number,patch:Partial<StationSlot>):void {onchange(item.slots.map((s,i)=>i===index?{...s,...patch}:s));}
  function segments(slot:StationSlot,day:number):{left:number;width:number}[] {
    if(!validSlot(slot))return [];
    const start=minutes(slot.startHm),end=minutes(slot.endHm);
    const length=end>start?end-start:1440-start+end;
    const ranges=[];
    for(let d=0;d<7;d++)if(slot.daysMask & (1<<d)) {
      for(const offset of [0,10080]) {
        const from=Math.max(0,d*1440+start-day*1440-offset);
        const to=Math.min(1440,d*1440+start+length-day*1440-offset);
        if(to>from)ranges.push({left:from/1440*100,width:(to-from)/1440*100});
      }
    }
    return ranges;
  }
  function key(event:KeyboardEvent,day:number,hour:number):void {
    let next=day*24+hour;
    if(event.key==='ArrowLeft')next=Math.max(0,next-1);
    else if(event.key==='ArrowRight')next=Math.min(167,next+1);
    else if(event.key==='ArrowUp')next=Math.max(0,next-24);
    else if(event.key==='ArrowDown')next=Math.min(167,next+24);
    else if(event.key==='Escape'){stop();event.stopPropagation();return;}
    else return;
    event.preventDefault();event.stopPropagation();focused=next;
    grid.querySelector<HTMLButtonElement>(`[data-index="${next}"]`)?.focus();
  }
</script>

<svelte:window onpointermove={pointerMove} onpointerup={pointerUp} onpointercancel={stop} onblur={stop}/>

<section class="week-picker" aria-label={t('station.schTitle')}>
  <div class="week-head"><p>{timezone??t('station.localTimezone')} · {t('station.gridHint')}</p></div>
  <div class="grid-scroll"><div class="week-grid" bind:this={grid} aria-label={t('station.dragSchedule')}>
    <div class="week-row hour-row"><span></span>{#each hours as hour (hour)}<span class="hour">{String(hour).padStart(2,'0')}</span>{/each}</div>
    {#each WEEK_DAYS as dayKey,day (dayKey)}
      <div class="week-row">
        <span class="day">{t(dayKey)}</span>
        {#each hours as hour (hour)}
          <button class="week-cell" data-index={day*24+hour} data-day={day} data-hour={hour}
            type="button" tabindex={focused===day*24+hour?0:-1} aria-label={`${t('station.weekday',{day:t(dayKey)})} ${hm(hour*60)}–${hm((hour+1)*60)}`}
            title={`${t('station.weekday',{day:t(dayKey)})} ${hm(hour*60)}–${hm((hour+1)*60)}`} {disabled} onkeydown={e=>key(e,day,hour)}
            onpointerdown={e=>{if(e.button!==0 || disabled)return;e.preventDefault();e.currentTarget.focus();grid.setPointerCapture(e.pointerId);focused=day*24+hour;anchor={day,hour};endpoint={day,hour};document.documentElement.classList.add('station-selecting');}}
            onclick={e=>{if(e.detail===0 && !disabled)onchange([...item.slots,range({day,hour},{day,hour})]);}}><span></span></button>
        {/each}
        <div class="range-track" aria-hidden="true">
          {#each items.filter(i=>i.playlistId!==item.playlistId && i.scheduled) as other (other.playlistId)}
            {#each other.slots as slot}{#each segments(slot,day) as band}<span class="time-band other" style:left={`${band.left}%`} style:width={`${band.width}%`}></span>{/each}{/each}
          {/each}
          {#each item.slots as slot,index (index)}{#each segments(slot,day) as band}<span class="time-band" data-slot={index} style:left={`${band.left}%`} style:width={`${band.width}%`}></span>{/each}{/each}
          {#if candidate}{#each segments(candidate,day) as band}<span class="time-band preview" style:left={`${band.left}%`} style:width={`${band.width}%`}></span>{/each}{/if}
        </div>
      </div>
    {/each}
  </div>
  </div>
  <div class="legend"><span><i class="current"></i>{t('station.currentSlot')}</span><span><i></i>{t('station.otherSlot')}</span></div>
  <div class="editor-head"><Button size="sm" {disabled} onclick={add}>{#snippet icon()}<Plus size={13}/>{/snippet}{t('station.newSlot')}</Button></div>
  <div class="slot-list">
    {#each item.slots as slot,index (index)}
      <div class="slot-row" class:invalid={!validSlot(slot)} role="group" aria-label={t('station.slotNumber',{number:index+1})}>
        <div class="slot-line">
          <div class="days">{#each WEEK_DAYS as dayKey,day (dayKey)}<Button size="sm" ariaLabel={t('station.weekday',{day:t(dayKey)})} variant={slot.daysMask & (1<<day)?'primary':'secondary'} pressed={!!(slot.daysMask & (1<<day))} {disabled} onclick={()=>update(index,{daysMask:slot.daysMask^(1<<day)})}>{t(dayKey)}</Button>{/each}</div>
          <div class="time-range">
            <Field label={t('station.schStart')} hideLabel value={slot.startHm} placeholder="HH:MM" maxlength={5} inputmode="numeric" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" inputFilter={timeInput} mono {disabled} oninput={v=>update(index,{startHm:v})}/>
            <span class="time-separator" aria-hidden="true">-</span>
            <Field label={t('station.schEnd')} hideLabel value={slot.endHm} placeholder="HH:MM" maxlength={5} inputmode="numeric" pattern="([01][0-9]|2[0-3]):[0-5][0-9]|24:00" inputFilter={timeInput} mono {disabled} oninput={v=>update(index,{endHm:v})}/>
          </div>
          <span class="remove-slot"><IconButton label={t('station.schDelete')} {disabled} onclick={()=>onchange(item.slots.filter((_,i)=>i!==index))}><X size={14}/></IconButton></span>
        </div>
      </div>
    {/each}
  </div>
  {#if !item.slots.length}<p class="empty-hint">{t('station.emptySlots')}</p>{/if}
</section>

<style>
  .week-picker {border-top:1px solid var(--line);padding-top:var(--space-4);margin-top:var(--space-4);}
  .week-head {text-align:center;margin-bottom:var(--space-3);}.week-head p {margin:var(--space-2) 0 0;font-size:var(--fs-sm);color:var(--text-2);line-height:1.6;}
  .grid-scroll {overflow-x:auto;overscroll-behavior-x:contain;padding:var(--space-1) 0;}
  .week-grid {min-width:38rem;max-width:48rem;margin:0 auto;user-select:none;touch-action:none;cursor:crosshair;}
  .week-row {display:grid;grid-template-columns:2rem repeat(24,minmax(0,1fr));position:relative;height:1.75rem;}.hour-row {height:1.5rem;align-items:center;}
  .hour {font-size:var(--fs-sm);font-variant-numeric:tabular-nums;text-align:center;color:var(--text-2);}.day {font-size:var(--fs-sm);align-self:center;color:var(--text-2);}
  .week-cell {min-width:0;position:relative;padding:0;background:none;border:0;cursor:inherit;touch-action:none;}.week-cell>span {position:absolute;inset:2px;border:1px solid var(--line-strong);border-radius:var(--radius-xs);background:var(--surface-input);pointer-events:none;}
  .week-cell:focus-visible {outline:2px solid var(--text-1);outline-offset:-2px;}
  .range-track {position:absolute;left:2rem;right:0;top:2px;bottom:2px;pointer-events:none;}.time-band {position:absolute;top:0;bottom:0;border-radius:var(--radius-xs);border:1px solid color-mix(in srgb,var(--accent) 65%,transparent);background:color-mix(in srgb,var(--accent) 28%,transparent);}.time-band.other {background:color-mix(in srgb,var(--text-2) 18%,transparent);border-color:color-mix(in srgb,var(--text-2) 30%,transparent);}.time-band.preview {background:color-mix(in srgb,var(--accent) 50%,transparent);border-color:var(--accent);}
  :global(html.station-selecting),:global(html.station-selecting *) {cursor:crosshair !important;user-select:none !important;}
  .legend {display:flex;justify-content:center;align-items:center;gap:var(--space-4);margin:var(--space-3) 0 var(--space-5);font-size:var(--fs-sm);color:var(--text-2);}.legend span {display:flex;align-items:center;gap:var(--space-1);}.legend i {width:8px;height:8px;border-radius:2px;background:var(--thumb);}.legend .current {background:color-mix(in srgb,var(--accent) 50%,transparent);}
  .editor-head {display:flex;align-items:center;gap:var(--space-3);margin-bottom:var(--space-3);}
  .slot-list {display:flex;flex-direction:column;}.slot-row {border-top:1px solid var(--line);padding:var(--space-4) 0;}.slot-row:last-child {padding-bottom:0;}.slot-row.invalid {border-top-color:var(--danger);}
  .slot-line {display:flex;align-items:center;gap:var(--space-3);overflow-x:auto;padding:var(--space-1);overscroll-behavior-x:contain;}.days {display:flex;gap:var(--space-1);flex:none;}
  .time-range {display:grid;grid-template-columns:4.75rem .625rem 4.75rem;align-items:center;gap:var(--space-2);flex:none;}.time-separator {text-align:center;font-size:var(--fs-sm);color:var(--text-2);}
  .empty-hint {font-size:var(--fs-sm);color:var(--text-2);line-height:1.6;}
  .remove-slot {display:flex;flex:none;margin-left:auto;}
</style>
