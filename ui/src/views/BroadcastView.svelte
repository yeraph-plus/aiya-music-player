<script lang="ts">
  import StationPanel from '../components/station/StationPanel.svelte';
</script>

<div class="page-pad broadcast"><StationPanel /></div>

<style>
  .broadcast { flex:1; min-height:0; overflow:auto; min-width:0;padding-bottom:var(--space-6); }
</style>
