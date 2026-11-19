<script lang="ts">
  import { FolderOpen } from '@lucide/svelte';
  import { backendCaps } from '../lib/backend-caps';
  import { errorMessage } from '../lib/errors';
  import { t } from '../lib/i18n.svelte';
  import { absoluteRoot, normalizeRoot, rootKey } from '../lib/library-path';
  import { library } from '../stores/library.svelte';
  import { scan } from '../stores/scan.svelte';
  import Button from './ui/Button.svelte';
  import Dialog from './ui/Dialog.svelte';
  import Field from './ui/Field.svelte';

  let { disabled=false, size='md', primary=false, label }: {
    disabled?:boolean;size?:'sm'|'md';primary?:boolean;label?:string;
  } = $props();
  const caps=backendCaps();
  let open=$state(false);
  let busy=$state(false);
  let draft=$state('');
  let error=$state<string|null>(null);
  let input:HTMLInputElement|null=$state(null);

  async function choose():Promise<void> {
    if(caps.pickFolder){await scan.pickRootAndScan();return;}
    draft='';error=null;open=true;
    await library.loadRoots();
  }
  async function save():Promise<void> {
    if(busy || disabled || library.scanning)return;
    const path=normalizeRoot(draft);
    if(!absoluteRoot(path)){error=t('library.tools.rootInvalid');return;}
    if([...library.roots,...library.pausedRoots].some(root=>rootKey(root)===rootKey(path))){
      error=t('library.tools.noticeRootExists');return;
    }
    busy=true;error=null;
    try {
      await library.saveRoots([...library.roots,path],library.pausedRoots);
      open=false;
    } catch(err){error=errorMessage(err);}
    finally{busy=false;}
  }
</script>

<Button {size} variant={primary?'primary':'secondary'} disabled={disabled || busy} onclick={()=>void choose()}>
  {#snippet icon()}<FolderOpen size={14}/>{/snippet}{label??t('library.tools.addFolder')}
</Button>
{#if open}
  <Dialog title={t('library.tools.addFolder')} description={t('library.tools.rootDialogHint')} width="md" dismissible={!busy} autofocus={()=>input?.focus()} onclose={()=>{if(!busy)open=false;}}>
    <div class="folder-form"><Field label={t('library.tools.rootPath')} value={draft} placeholder={t('library.tools.rootPlaceholder')} {error} mono disabled={busy} bind:ref={input} oninput={value=>{draft=value;error=null;}} onenter={()=>void save()}/></div>
    {#snippet actions()}
      <Button disabled={busy} onclick={()=>open=false}>{t('common.cancel')}</Button>
      <Button variant="primary" loading={busy} disabled={!draft.trim() || disabled || library.scanning} onclick={()=>void save()}>{t('library.tools.addAndWatch')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>.folder-form {padding:0;}</style>
