<script lang="ts">
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { history } from '../stores/history.svelte';
  import { ui } from '../stores/ui.svelte';
</script>

<SettingsPanel
  settings={ui.settings}
  diagnostics={ui.diagnostics}
  beets={ui.beets}
  onsave={(next) => ui.applySettings(next)}
  onrefresh={() => void ui.refreshDiagnostics()}
  onclearhistory={() => void history.clear()}
  onopendatadir={() => ui.openDataDir()}
/>
