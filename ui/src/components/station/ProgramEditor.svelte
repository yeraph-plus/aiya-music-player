<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { artworkUrl } from '../../lib/artwork';
  import { LIBRARY_SOURCE, queueItemError } from '../../lib/station-time';
  import type { PlaylistSummary, StationProgram, StationQueueItem, StationSlot } from '../../lib/ipc/generated';
  import IconButton from '../ui/IconButton.svelte';
  import Card from '../ui/Card.svelte';
  import Select from '../ui/Select.svelte';
  import Switch from '../ui/Switch.svelte';
  import Field from '../ui/Field.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import WeekTimePicker from './WeekTimePicker.svelte';
  import { Plus, Check, ArrowUp, ArrowDown, X } from '@lucide/svelte';

  let { items=$bindable(), valid=$bindable(true), playlists, program, disabled=false }: {
    items:StationQueueItem[];valid?:boolean;playlists:PlaylistSummary[];program:StationProgram;disabled?:boolean;
  } = $props();
  const sources=$derived([{id:LIBRARY_SOURCE,name:t('station.allLibrary'),kind:'library',trackCount:program.sources.find(p=>p.playlistId===LIBRARY_SOURCE)?.trackCount??0},...playlists]);
  const errors=$derived(items.map(item=>queueItemError(item,items)));
  $effect(()=>{valid=!errors.some(Boolean);});
  const name=(id:string):string=>sources.find(p=>p.id===id)?.name ?? program.sources.find(p=>p.playlistId===id)?.name ?? id;
  const cover=(id:string):string|null=>artworkUrl(playlists.find(p=>p.id===id)?.artworkId??null,256);
  async function add(id:string):Promise<void> {
    if(!items.some(i=>i.playlistId===id))items=[...items,{playlistId:id,playMode:'ordered',priority:0,scheduled:false,slots:[]}];
    await tick();document.getElementById(`program-${id}`)?.scrollIntoView({block:'nearest'});
  }
  function change(id:string,patch:Partial<StationQueueItem>):void {items=items.map(i=>i.playlistId===id?{...i,...patch}:i);}
  function move(index:number,delta:number):void {const next=[...items];[next[index],next[index+delta]]=[next[index+delta],next[index]];items=next;}
</script>

<section class="playlist-library" aria-label={t('station.systemPlaylists')}>
  <h3>{t('station.systemPlaylists')}<span>{sources.length}</span></h3>
  <div class="playlist-grid">
    {#each sources as playlist (playlist.id)}
      {@const added=items.some(i=>i.playlistId===playlist.id)}
      <Card title={playlist.name} subtitle={`${t(playlist.kind==='library'?'station.librarySource':playlist.kind==='smart'?'station.smart':'station.manual')} · ${t(playlist.trackCount===1?'station.oneTrack':'station.plTracks',{count:playlist.trackCount})}`} cover={cover(playlist.id)}>
        {#snippet actions()}<IconButton label={t(added?'station.addedPlaylist':'station.addPlaylist',{name:playlist.name})} {disabled} onclick={()=>void add(playlist.id)}>{#if added}<Check size={17}/>{:else}<Plus size={17}/>{/if}</IconButton>{/snippet}
      </Card>
    {/each}
  </div>
</section>

<section class="program-queue" aria-label={t('station.broadcastQueue')}>
  <h3>{t('station.broadcastQueue')}<span>{items.length}</span></h3>
  <p class="section-hint">{t('station.queueHint')}</p>
  <ol class="queue-list">
    {#each items as item,index (item.playlistId)}
      {@const error=errors[index]}
      <li id={`program-${item.playlistId}`}>
        <Card title={`${index+1}. ${name(item.playlistId)}`} cover={cover(item.playlistId)} subtitle={t(item.scheduled?'station.scheduled':'station.regular')}>
          {#snippet actions()}
            <IconButton label={t('station.moveUp')} disabled={disabled || index===0} onclick={()=>move(index,-1)}><ArrowUp size={15}/></IconButton>
            <IconButton label={t('station.moveDown')} disabled={disabled || index===items.length-1} onclick={()=>move(index,1)}><ArrowDown size={15}/></IconButton>
            <IconButton label={t('station.removeQueueItem',{name:name(item.playlistId)})} {disabled} onclick={()=>items=items.filter(i=>i.playlistId!==item.playlistId)}><X size={15}/></IconButton>
          {/snippet}
          <div class="queue-controls">
            <div class="order-field"><span>{t('station.playOrder')}</span><Select label={t('station.playOrder')} value={item.playMode} options={[{value:'ordered',label:t('station.ordered')},{value:'shuffle',label:t('station.shuffle')}]} {disabled} onchange={v=>change(item.playlistId,{playMode:v as StationQueueItem['playMode']})}/></div>
            <div class="priority-field"><Field label={t('station.schPriority')} type="number" value={String(item.priority)} min={0} max={65535} step={1} {disabled} oninput={v=>change(item.playlistId,{priority:v===''?-1:Number(v)})}/></div>
            <div class="schedule-switch"><span>{t('station.scheduled')}</span><Switch label={t('station.scheduled')} checked={item.scheduled} {disabled} onchange={v=>change(item.playlistId,{scheduled:v})}/></div>
          </div>
          {#if item.scheduled}<WeekTimePicker {item} {items} timezone={program.timezone} {disabled} onchange={(slots:StationSlot[])=>change(item.playlistId,{slots})}/>
          {/if}
          {#if error}<p class="program-error" role="alert">{t(error)}</p>{/if}
        </Card>
      </li>
    {/each}
  </ol>
  {#if !items.length}<EmptyState title={t('station.emptyQueue')}/>{/if}
</section>

<style>
  h3 {font-size:var(--fs-md);font-weight:600;margin:0 0 var(--space-3);display:flex;align-items:center;gap:var(--space-2);}h3>span {font-size:var(--fs-sm);font-weight:400;color:var(--text-2);}
  .playlist-library {margin-bottom:var(--space-6);}.playlist-grid {display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,15.625rem),1fr));gap:var(--space-3);}
  .section-hint {font-size:var(--fs-sm);color:var(--text-2);line-height:1.6;}
  .section-hint {margin:0 0 var(--space-3);}
  .queue-list {list-style:none;padding:0;margin:0;display:flex;flex-direction:column;gap:var(--space-4);}
  .queue-list>li {min-width:0;scroll-margin-top:var(--space-4);}
  .queue-controls {display:flex;align-items:end;gap:var(--space-4);flex-wrap:wrap;}
  .order-field {display:flex;flex-direction:column;gap:var(--space-2);width:11.25rem;}.priority-field {width:6.875rem;}
  .order-field>span,.schedule-switch>span {font-size:var(--fs-sm);color:var(--text-2);}.schedule-switch {display:flex;align-items:center;gap:var(--space-3);height:var(--control-h);}
  .program-error {font-size:var(--fs-sm);color:var(--danger);}
</style>
