<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { programIssueText } from '../../lib/station-i18n';
  import { stationManager as manager } from '../../stores/station-manager.svelte';
  import { notify } from '../../stores/notify.svelte';
  import { nav } from '../../stores/nav.svelte';
  import type { StationQueueItem, StationSummary } from '../../lib/ipc/generated';
  import Header from '../content/Header.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Dialog from '../ui/Dialog.svelte';
  import Card from '../ui/Card.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import StationFields from './StationFields.svelte';
  import ProgramEditor from './ProgramEditor.svelte';
  import { Plus, Settings2, RadioTower, Play, Square, SkipForward, Copy, Pencil, Trash2, ArrowLeft } from '@lucide/svelte';

  const blank=()=>({name:'',subtitle:'',description:'',coverUrl:'',visibility:'private'});
  const clone=<T,>(value:T):T=>JSON.parse(JSON.stringify(value));
  let selected=$state<string|null>(null);
  let busy=$state(false);
  let valid=$state(true);
  let items=$state<StationQueueItem[]>([]);
  let baseline=$state('');
  let revision=$state('0');
  let form=$state(blank());
  let editing=$state<'new'|'edit'|null>(null);
  let deleting=$state(false);
  let pending=$state<(()=>void)|null>(null);
  let cancelDelete:HTMLButtonElement|null=$state(null);
  const detail=$derived(manager.detail?.id===selected?manager.detail:null);
  const dirty=$derived(!!selected && !!baseline && JSON.stringify(items)!==baseline);
  const url=(station:StationSummary):string=>new URL(station.hlsUrl,location.origin).href;
  const visibility=(station:StationSummary):string=>t(station.visibility==='public'?'station.publicShort':'station.privateShort');
  const visibilityBadge=(station:StationSummary):string=>t('station.visibilityBadge',{visibility:visibility(station)});

  function capture():void {
    if(!detail)return;
    items=clone(detail.program.items);baseline=JSON.stringify(items);revision=detail.program.revision;
  }
  function allow(action:()=>void):boolean {
    if(busy)return false;
    if(dirty){pending=action;return false;}
    return true;
  }
  function request(action:()=>void):void {if(allow(action))action();}
  async function open(id:string|null):Promise<void> {
    const old=selected;selected=id;baseline='';
    if(await manager.load(id))capture();
    await tick();
    document.querySelector('.broadcast')?.scrollTo({top:0});
    if(!id)document.querySelector<HTMLButtonElement>(`[data-station="${old}"] .configure .k-icon-btn`)?.focus();
  }
  function edit(mode:'new'|'edit'):void {
    form=mode==='edit' && detail?{name:detail.name,subtitle:detail.subtitle??'',description:detail.description??'',coverUrl:detail.coverUrl??'',visibility:detail.visibility}:blank();
    editing=mode;
  }
  async function saveInfo():Promise<void> {
    if(busy || !form.name.trim())return;
    busy=true;
    try {
      const mode=editing;
      const result=await manager.save(mode==='new'?null:selected,{name:form.name.trim(),subtitle:form.subtitle.trim()||null,description:form.description.trim()||null,coverUrl:form.coverUrl.trim()||null,visibility:form.visibility});
      if(result){editing=null;if(mode==='new')await open(result.id);else await manager.load(selected);}
    } finally {busy=false;}
  }
  async function save():Promise<boolean> {
    if(!detail || busy || !valid)return false;
    busy=true;
    try {
      if(!(await manager.setProgram(detail.id,{revision,items:clone(items)})))return false;
      await manager.load(selected);capture();notify.showToast(t('station.savedPending'));return true;
    } finally {busy=false;}
  }
  async function activate(station:StationSummary):Promise<void> {
    if(busy)return;
    busy=true;
    try {await manager.setActive(station.id,!station.active);await manager.load(selected);}
    finally {busy=false;}
  }
  async function copy(station:StationSummary):Promise<void> {
    try {await navigator.clipboard.writeText(url(station));notify.showToast(t('station.copied'));}
    catch {notify.showError(t('station.copyFailed'));}
  }
  async function apply():Promise<void> {
    if(!selected || busy)return;
    busy=true;
    try {if(await manager.apply(selected))notify.showToast(t('station.applied'));await manager.load(selected);}
    finally {busy=false;}
  }
  async function remove():Promise<void> {
    if(!selected || busy)return;
    busy=true;
    try {if(await manager.remove(selected)){deleting=false;await open(null);}}
    finally {busy=false;}
  }
  async function refresh():Promise<void> {
    if(busy || manager.loading || editing)return;
    await manager.load(selected);if(!dirty && detail)capture();
  }
  onMount(()=>{
    void manager.load(null);nav.beforeLeave=allow;
    const timer=setInterval(()=>void refresh(),5000);
    return ()=>{clearInterval(timer);nav.beforeLeave=null;};
  });
</script>

<svelte:window onbeforeunload={event=>{if(dirty){event.preventDefault();event.returnValue='';}}}/>

<div class="station">
  <Header kicker="BROADCAST" title={t('station.pageTitle')} subtitle={t('station.pageSubtitle')}>
    {#snippet actions()}{#if !selected}<Button variant="primary" disabled={busy} onclick={()=>edit('new')}>{#snippet icon()}<Plus size={14}/>{/snippet}{t('station.new')}</Button>{/if}{/snippet}
  </Header>
  {#if selected}
    <div class="workbench-toolbar">
      <Button disabled={busy} onclick={()=>request(()=>void open(null))}>{#snippet icon()}<ArrowLeft size={14}/>{/snippet}{t('station.back')}</Button>
      <Button disabled={busy || !detail} onclick={()=>edit('edit')}>{#snippet icon()}<Pencil size={14}/>{/snippet}{t('station.editInfo')}</Button>
      <Button variant="danger" disabled={busy || !detail} onclick={()=>deleting=true}>{#snippet icon()}<Trash2 size={14}/>{/snippet}{t('station.del')}</Button>
      {#if detail}
        <div class="workbench-save">
          <span class="program-status" class:pending={dirty || detail.program.needsApply}>{t(dirty?'station.unsaved':detail.program.needsApply?'station.savedPending':'station.upToDate')}</span>
          {#if dirty}<Button variant="ghost" disabled={busy} onclick={capture}>{t('station.cancel')}</Button><Button variant="primary" loading={busy} disabled={!valid} onclick={()=>void save()}>{t('station.saveConfig')}</Button>
          {:else if detail.active && detail.program.needsApply}<Button variant="primary" title={t('station.applyHint')} loading={busy} disabled={!!detail.program.issues.length} onclick={()=>void apply()}>{t('station.apply')}</Button>{/if}
        </div>
      {/if}
    </div>
    {#if detail}
      <div class="workbench-heading"><h2>{detail.name}<span>{visibilityBadge(detail)}</span></h2><p>{detail.description||t('station.workbenchSubtitle')}</p></div>
      <section class="station-workbench" aria-label={t('station.configFor',{name:detail.name})}>
        <ProgramEditor bind:valid bind:items playlists={manager.playlists} program={detail.program} disabled={busy}/>
        {#if detail.program.error && !detail.program.issues.length}<p class="program-error" role="alert">{detail.program.error}</p>{/if}
        {#if detail.program.issues.length && !dirty}<ul class="program-issues">{#each detail.program.issues as issue}<li>{programIssueText(issue,detail.program.sources)}</li>{/each}</ul>{/if}
      </section>
    {:else}<p class="loading">{t('station.loading')}</p>{/if}
  {:else}
    {#if manager.loading && !manager.stations.length}<p class="loading">{t('station.loading')}</p>
    {:else if !manager.stations.length}<EmptyState title={t('station.empty')}>{#snippet icon()}<RadioTower size={26}/>{/snippet}</EmptyState>{/if}
    <div class="station-list">
      {#each manager.stations as station (station.id)}
        <article data-station={station.id}>
          <Card title={`${station.name}${visibilityBadge(station)}`} cover={station.coverUrl}>
            {#snippet metadata()}<a class="listen-url" href={url(station)} target="_blank" rel="noreferrer">{url(station)}</a>{/snippet}
            {#snippet actions()}
              <span class="on-air" class:running={station.running} role="img" aria-label={t(station.running?'station.stateOn':'station.stateOff')} title={t(station.running?'station.stateOn':'station.stateOff')}></span>
              <IconButton label={t(station.active?'station.stop':'station.play')} disabled={busy} onclick={()=>void activate(station)}>{#if station.active}<Square size={14}/>{:else}<Play size={14}/>{/if}</IconButton>
              <IconButton label={t('station.skip')} disabled={busy || !station.running} onclick={()=>void manager.skip(station.id)}><SkipForward size={16}/></IconButton>
              <IconButton label={t('station.copyHls')} disabled={busy} onclick={()=>void copy(station)}><Copy size={16}/></IconButton>
              <span class="configure"><IconButton label={t('station.configure')} disabled={busy} onclick={()=>void open(station.id)}><Settings2 size={16}/></IconButton></span>
            {/snippet}
          </Card>
        </article>
      {/each}
    </div>
  {/if}
</div>

{#if editing}
  <Dialog title={t(editing==='new'?'station.formTitleNew':'station.editInfo')} description={t('station.formHint')} width="md" onclose={()=>{if(!busy)editing=null;}}>
    <StationFields bind:value={form} disabled={busy}/>
    {#snippet actions()}<Button disabled={busy} onclick={()=>editing=null}>{t('station.cancel')}</Button><Button variant="primary" loading={busy} disabled={!form.name.trim()} onclick={()=>void saveInfo()}>{t('station.save')}</Button>{/snippet}
  </Dialog>
{/if}
{#if deleting && detail}
  <Dialog title={t('station.del')} description={t('station.deleteStation',{name:detail.name})} autofocus={()=>cancelDelete?.focus()} onclose={()=>{if(!busy)deleting=false;}}>
    {#snippet actions()}<Button bind:ref={cancelDelete} disabled={busy} onclick={()=>deleting=false}>{t('station.cancel')}</Button><Button variant="danger" loading={busy} onclick={()=>void remove()}>{t('station.del')}</Button>{/snippet}
  </Dialog>
{/if}
{#if pending}
  <Dialog title={t('station.unsaved')} description={t('station.unsavedHint')} onclose={()=>pending=null}>
    {#snippet actions()}
      <Button disabled={busy} onclick={()=>pending=null}>{t('station.keepEditing')}</Button>
      <Button disabled={busy} onclick={()=>{const action=pending;capture();pending=null;action?.();}}>{t('station.discard')}</Button>
      <Button variant="primary" loading={busy} disabled={!valid} onclick={async()=>{const action=pending;if(await save()){pending=null;action?.();}}}>{t('station.save')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .station-list {display:flex;flex-direction:column;gap:var(--space-3);}
  .listen-url {color:var(--text-2);text-decoration:none;font-family:var(--font-mono);font-size:var(--fs-sm);}
  .listen-url:hover {color:var(--accent-text);text-decoration:underline;}
  .workbench-toolbar {display:flex;gap:var(--space-2);align-items:center;margin-bottom:var(--space-5);flex-wrap:wrap;}.workbench-save {margin-left:auto;display:flex;align-items:center;gap:var(--space-2);}.program-status {font-size:var(--fs-sm);color:var(--text-2);}
  .workbench-heading {margin-bottom:var(--space-5);}.workbench-heading h2 {font-size:var(--fs-md);font-weight:600;margin:0;}.workbench-heading h2 span {font-size:var(--fs-sm);font-weight:400;color:var(--text-2);}.workbench-heading p {font-size:var(--fs-sm);color:var(--text-2);margin:var(--space-2) 0 0;}
  .on-air {width:7px;height:7px;border-radius:50%;background:var(--danger);margin-right:var(--space-1);flex:none;}.on-air.running {background:var(--ok);}.configure {display:flex;}
  .pending,.program-issues {color:var(--warn);}
  .program-issues {padding-left:var(--space-4);font-size:var(--fs-sm);line-height:1.7;}
  .program-error {font-size:var(--fs-base);color:var(--danger);}.loading {font-size:var(--fs-base);color:var(--text-2);}
</style>
