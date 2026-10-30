<script lang="ts">
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 设置页只剩一个薄壳：真正的内容在 `SettingsPanel`（外观 / 播放 / 曲库 /
   * 系统 / 快捷键 / 关于六组）。
   *
   * 2026-10-30：docs/29 批 A 拆出去的「扫描入库」页并入这里——那一页在 beets
   * 拆成独立「整理库」页（docs/48）之后只剩"曲库怎么进来"一件事，一条目录
   * 卡片撑不起一个一级席位。现在曲库目录、扫描、执行状态都在「曲库」组。
   */
</script>

<SettingsPanel settings={ui.settings} onsave={(next) => ui.applySettings(next)} />
