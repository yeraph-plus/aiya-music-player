<script lang="ts">
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * docs/29 批 A：诊断 / beets / 清空历史等回调随分区搬去 ScanToolsView，
   * 设置页只剩纯配置（外观/播放/系统/快捷键），只保留 settings 管线。
   */
</script>

<SettingsPanel settings={ui.settings} onsave={(next) => ui.applySettings(next)} />
