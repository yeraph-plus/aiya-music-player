<script lang="ts">
  import { t } from '../../lib/i18n.svelte';
  import Field from '../ui/Field.svelte';
  import Select from '../ui/Select.svelte';

  let { value=$bindable(), disabled=false }: {
    value:{name:string;subtitle:string;description:string;coverUrl:string;visibility:string};disabled?:boolean;
  } = $props();
</script>

<div class="station-fields">
  <Field label={t('station.nameRequired')} value={value.name} placeholder={t('station.fieldNamePh')} {disabled} required maxlength={160} oninput={v=>value.name=v}/>
  <div class="visibility-field"><span>{t('station.visibilityRequired')}</span>
    <Select label={t('station.fieldVisibility')} value={value.visibility} options={[{value:'private',label:t('station.visPrivate')},{value:'public',label:t('station.visPublic')}]} {disabled} onchange={v=>value.visibility=v}/>
  </div>
  <Field label={t('station.fieldSubtitle')} value={value.subtitle} {disabled} oninput={v=>value.subtitle=v}/>
  <Field label={t('station.fieldCover')} type="url" value={value.coverUrl} placeholder="https://…" {disabled} oninput={v=>value.coverUrl=v}/>
  <div class="wide"><Field label={t('station.fieldDesc')} value={value.description} {disabled} oninput={v=>value.description=v}/></div>
</div>

<style>
  .station-fields {display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:var(--space-4);padding:var(--space-4) 16px 0;}
  .visibility-field {display:flex;flex-direction:column;gap:var(--space-2);min-width:0;}
  .visibility-field>span {font-size:var(--fs-sm);color:var(--text-2);}
  .wide {grid-column:1/-1;}
</style>
