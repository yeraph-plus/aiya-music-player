<script lang="ts">
  import type { AppSettings, BeetsStatus, ShortcutAction } from '@aiya/contracts';
  import type { Diagnostics } from '../lib/ipc/generated/Diagnostics';
  import {
    SHORTCUT_ACTIONS,
    SHORTCUT_LABELS,
    findConflicts,
    formatBinding,
    resolveShortcutBindings,
  } from '@aiya/contracts';
  import { bindingFromEvent } from '../lib/keys';
  import { formatBytes, formatDateTime, formatDurationLong, shortenPath } from '../lib/format';
  import Icon from './Icon.svelte';

  interface Props {
    settings: AppSettings;
    diagnostics: Diagnostics | null;
    beets: BeetsStatus | null;
    onclose: () => void;
    onsave: (next: AppSettings) => void;
    onrefresh: () => void;
    onclearhistory: () => void;
    onopendatadir: () => void;
  }

  let {
    settings,
    diagnostics,
    beets,
    onclose,
    onsave,
    onrefresh,
    onclearhistory,
    onopendatadir,
  }: Props =
    $props();

  /**
   * 诊断页两张表的数据形状。
   *
   * 为什么要在前端定义：Rust `Diagnostics` 的 `list_timings` / `recent_errors` /
   * `shortcut_conflicts` 是 `Vec<serde_json::Value>`，ts-rs 只能导出 `unknown[]`，
   * 而这三个数组在 `src-tauri/src/lib.rs` 里**恒为 `Vec::new()`**（还没实现采集）。
   * 所以这里按 UI 实际消费的字段声明视图类型 + 一次显式转换：
   * 空数组转换成空数组是无害的；等后端真的填数据时，补 DTO 并把这两行删掉
   * （记在 docs/18 §9 的遗留项里）。
   */
  type TimingRow = { at: number; source: string; rows: number; total: number; tookMs: number };
  type ErrorRow = { at: number; message: string };

  const listTimings = $derived((diagnostics?.listTimings ?? []) as TimingRow[]);
  const recentErrors = $derived((diagnostics?.recentErrors ?? []) as ErrorRow[]);

  type Tab = 'appearance' | 'playback' | 'system' | 'shortcuts' | 'diagnostics';
  let tab = $state<Tab>('appearance');
  let capturing = $state<ShortcutAction | null>(null);

  const bindings = $derived(resolveShortcutBindings(settings.shortcuts));
  const conflicts = $derived(findConflicts(bindings));

  function apply(patch: Partial<AppSettings>): void {
    onsave({ ...settings, ...patch });
  }

  function setShortcut(action: ShortcutAction, binding: string): void {
    const shortcuts = { ...settings.shortcuts };
    if (!binding) delete shortcuts[action];
    else shortcuts[action] = binding;
    apply({ shortcuts });
  }

  function onCaptureKey(event: KeyboardEvent): void {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();

    if (event.key === 'Escape') {
      capturing = null;
      return;
    }
    if (event.key === 'Backspace' || event.key === 'Delete') {
      setShortcut(capturing, '');
      capturing = null;
      return;
    }

    const binding = bindingFromEvent(event);
    if (!binding) return;
    setShortcut(capturing, binding);
    capturing = null;
  }

  /** 模态 Escape 语义：录制快捷键时交给 onCaptureKey（Esc 只取消录制）；输入态只失焦；其余关闭面板 */
  function onWindowKey(event: KeyboardEvent): void {
    if (capturing) {
      onCaptureKey(event);
      return;
    }
    if (event.key !== 'Escape') return;
    const el = event.target as HTMLElement | null;
    if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) {
      el.blur();
      return;
    }
    onclose();
  }

  const TABS: Array<{ id: Tab; label: string }> = [
    { id: 'appearance', label: '外观' },
    { id: 'playback', label: '播放' },
    { id: 'system', label: '系统' },
    { id: 'shortcuts', label: '快捷键' },
    { id: 'diagnostics', label: '诊断' },
  ];
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="modal-backdrop" role="presentation" onclick={onclose}></div>
<div class="modal settings" role="dialog" aria-label="设置">
  <header>
    <strong>设置</strong>
    <div class="tabs">
      {#each TABS as item (item.id)}
        <button class="chip" class:on={tab === item.id} onclick={() => (tab = item.id)}>{item.label}</button>
      {/each}
    </div>
    <button class="icon-btn" onclick={onclose} aria-label="关闭设置"><Icon name="close" size={14} /></button>
  </header>

  <div class="modal-body">
    {#if tab === 'appearance'}
      <div class="setting-row">
        <div>
          <div class="setting-title">主题</div>
          <div class="setting-hint">浅色主题适合白天与投影，切换即时生效</div>
        </div>
        <div class="segmented">
          <button class:on={settings.theme === 'dark'} onclick={() => apply({ theme: 'dark' })}>
            <Icon name="moon" size={13} /> 深色
          </button>
          <button class:on={settings.theme === 'light'} onclick={() => apply({ theme: 'light' })}>
            <Icon name="sun" size={13} /> 浅色
          </button>
        </div>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">列表密度</div>
          <div class="setting-hint">紧凑模式一屏多显示约 20%，适合大曲库浏览</div>
        </div>
        <div class="segmented">
          <button class:on={settings.density === 'comfortable'} onclick={() => apply({ density: 'comfortable' })}>
            标准
          </button>
          <button class:on={settings.density === 'compact'} onclick={() => apply({ density: 'compact' })}>
            紧凑
          </button>
        </div>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">封面主色</div>
          <div class="setting-hint">从封面提取强调色；关掉则使用固定蓝色</div>
        </div>
        <button class="switch" class:on={settings.accentFromArtwork} onclick={() => apply({ accentFromArtwork: !settings.accentFromArtwork })} aria-label="封面主色">
          <i></i>
        </button>
      </div>
    {:else if tab === 'playback'}
      <div class="setting-row">
        <div>
          <div class="setting-title">启动时自动播放</div>
          <div class="setting-hint">打开应用就接着放上次那一首；关闭则停在暂停状态</div>
        </div>
        <button class="switch" class:on={settings.autoplayOnStart} onclick={() => apply({ autoplayOnStart: !settings.autoplayOnStart })} aria-label="启动时自动播放">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">记住播放进度</div>
          <div class="setting-hint">下次启动从上次停下的位置继续；关闭则每次从头开始</div>
        </div>
        <button class="switch" class:on={settings.rememberPosition} onclick={() => apply({ rememberPosition: !settings.rememberPosition })} aria-label="记住播放进度">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">播放时阻止系统休眠</div>
          <div class="setting-hint">只阻止系统睡眠，屏幕仍会按系统设置熄掉</div>
        </div>
        <button class="switch" class:on={settings.preventSleep} onclick={() => apply({ preventSleep: !settings.preventSleep })} aria-label="播放时阻止系统休眠">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">音量归一化</div>
          <div class="setting-hint">
            按 ReplayGain 标签换算音量系数。近似实现：不改变你设置的音量，只做播放时的补偿
          </div>
        </div>
        <button class="switch" class:on={settings.normalizeVolume} onclick={() => apply({ normalizeVolume: !settings.normalizeVolume })} aria-label="音量归一化">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">频谱可视化</div>
          <div class="setting-hint">
            播放条出现频谱按钮，实时显示正在播放的声音（ADR-037：开启后会启用渲染层音频图）
          </div>
        </div>
        <button class="switch" class:on={settings.spectrum} onclick={() => apply({ spectrum: !settings.spectrum })} aria-label="频谱可视化">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">频谱风格</div>
          <div class="setting-hint">
            专业：audioMotion 渲染，面板头部可循环切换径向 / LED / 镜像等形态；简洁：自研轻量条形
          </div>
        </div>
        <div class="segmented">
          <button class:on={settings.spectrumEngine === 'pro'} onclick={() => apply({ spectrumEngine: 'pro' })}>
            专业
          </button>
          <button class:on={settings.spectrumEngine === 'compact'} onclick={() => apply({ spectrumEngine: 'compact' })}>
            简洁
          </button>
        </div>
      </div>
    {:else if tab === 'system'}
      <div class="setting-row">
        <div>
          <div class="setting-title">关闭窗口时最小化到托盘</div>
          <div class="setting-hint">
            打开后点关闭只是隐藏窗口，播放继续；要退出请用托盘菜单或 Ctrl+Q
          </div>
        </div>
        <button class="switch" class:on={settings.closeToTray} onclick={() => apply({ closeToTray: !settings.closeToTray })} aria-label="关闭窗口时最小化到托盘">
          <i></i>
        </button>
      </div>

      <div class="setting-row">
        <div>
          <div class="setting-title">系统媒体键</div>
          <div class="setting-hint">
            播放器的标题、封面与播放状态同步到系统媒体面板，媒体键由系统路由，失焦也生效
          </div>
        </div>
        <button class="switch" class:on={settings.mediaKeys} onclick={() => apply({ mediaKeys: !settings.mediaKeys })} aria-label="系统媒体键">
          <i></i>
        </button>
      </div>
    {:else if tab === 'shortcuts'}
      {#if conflicts.length > 0}
        <div class="notice danger">
          {#each conflicts as conflict (conflict.binding)}
            <div>
              {formatBinding(conflict.binding)} 同时绑定了
              {conflict.actions.map((action) => SHORTCUT_LABELS[action as ShortcutAction]).join('、')}
            </div>
          {/each}
        </div>
      {/if}

      <div class="shortcut-list">
        {#each SHORTCUT_ACTIONS as action (action)}
          <div class="shortcut-row">
            <span class="shortcut-label">{SHORTCUT_LABELS[action]}</span>
            <button
              class="binding"
              class:capturing={capturing === action}
              class:unset={!bindings[action]}
              onclick={() => (capturing = action)}
            >
              {capturing === action ? '按下新键…' : formatBinding(bindings[action])}
            </button>
          </div>
        {/each}
      </div>
      <div class="setting-foot">
        <span class="dim">Esc 取消录制，Backspace 清空绑定。↑/↓ 与 Enter 是列表导航，不占用快捷键。</span>
        <button class="btn" onclick={() => apply({ shortcuts: {} })}>恢复默认</button>
      </div>
    {:else}
      <div class="diag-actions">
        <button class="btn" onclick={onrefresh}><Icon name="refresh" size={13} /> 刷新</button>
        <button class="btn" onclick={onopendatadir}>打开数据目录</button>
        <button class="btn" onclick={onclearhistory}>清空播放历史</button>
      </div>

      {#if diagnostics}
        <div class="diag-grid">
          <div><span>版本</span><b>{diagnostics.version}</b></div>
          <div><span>Electron</span><b>{diagnostics.electron}</b></div>
          <div><span>平台</span><b>{diagnostics.platform}</b></div>
          <div><span>Schema</span><b>v{diagnostics.schemaVersion}</b></div>
          <div><span>曲库</span><b>{diagnostics.tracks} 首</b></div>
          <div><span>时长</span><b>{formatDurationLong(diagnostics.totalDurationMs)}</b></div>
          <div><span>专辑 / 艺人</span><b>{diagnostics.albums} / {diagnostics.artists}</b></div>
          <div><span>播放列表</span><b>{diagnostics.playlists}</b></div>
          <div><span>历史记录</span><b>{diagnostics.historyRows}</b></div>
          <div><span>数据库</span><b>{formatBytes(diagnostics.dbBytes)}</b></div>
          <div class="wide"><span>数据目录</span><b class="path" title={diagnostics.dataDir}>{shortenPath(diagnostics.dataDir)}</b></div>
        </div>

        <div class="diag-block">
          <div class="diag-head">
            媒体流
            <span class:danger={diagnostics.streams.active !== 0}>
              打开 {diagnostics.streams.opened} / 关闭 {diagnostics.streams.closed} / 未关闭 {diagnostics.streams.active}
            </span>
          </div>
          <div class="dim">连续切歌后"未关闭"应始终为 0；不为 0 说明协议层有泄漏。</div>
        </div>

        <div class="diag-block">
          <div class="diag-head">
            系统媒体控制
            <span>
              {#if !diagnostics.mediaKeys.enabled}
                已关闭
              {:else if diagnostics.mediaKeys.active.length > 0}
                已启用
              {:else}
                未生效
              {/if}
            </span>
          </div>
          <div class="dim">
            标题、封面与播放状态会同步到 Windows 媒体面板 / 锁屏 / 蓝牙耳机；
            只在播放时出现会话。
          </div>
        </div>

        <div class="diag-block">
          <div class="diag-head">
            系统集成
            <span>防休眠 {diagnostics.power.active ? '已拦截' : '未拦截'} · 托盘 {diagnostics.tray ? '已创建' : '未创建'}</span>
          </div>
          <div class="dim">
            播放中不应休眠（允许熄屏）；托盘图标只在"关闭窗口时最小化到托盘"打开时存在。
          </div>
        </div>

        <div class="diag-block">
          <div class="diag-head">
            beets 集成
            {#if beets === null}
              <span class="dim">探测中…</span>
            {:else if beets.found}
              <span>beets {beets.beetsVersion} · Python {beets.pythonVersion}</span>
            {:else}
              <span class="danger">未找到 beets</span>
            {/if}
          </div>
          {#if beets?.found}
            <div class="diag-grid">
              <div class="wide"><span>曲库</span><b class="path" title={beets.libraryPath ?? ''}>{beets.libraryExists ? shortenPath(beets.libraryPath ?? '') : '尚未创建（首次导入时生成）'}</b></div>
              <div class="wide"><span>插件</span><b>{beets.plugins?.length ? beets.plugins.join(', ') : '无'}</b></div>
              <div class="wide"><span>配置目录</span><b class="path" title={beets.configDir ?? ''}>{shortenPath(beets.configDir ?? '')}</b></div>
            </div>
            <div class="dim">标签匹配/候选/diff 由 beets 完成（写权威）；未安装时播放功能不受影响。</div>
          {:else if beets}
            <div class="dim">
              安装后重启应用即可启用导入整理功能：
              <code>pipx install beets</code> 或 <code>uv tool install beets</code>（推荐），
              或 <code>pip install beets</code>。
              {#if beets.errorMessage}
                <br /><span class="danger">{beets.errorMessage}</span>
              {/if}
            </div>
          {/if}
        </div>

        <div class="diag-block">
          <div class="diag-head">最近查询<span class="dim">越多越慢时说明该看索引了</span></div>
          {#if listTimings.length === 0}
            <div class="dim">还没有记录</div>
          {:else}
            <table class="diag-table">
              <thead><tr><th>时间</th><th>来源</th><th>行数</th><th>总数</th><th>耗时</th></tr></thead>
              <tbody>
                {#each listTimings.slice(0, 8) as timing (timing.at + String(timing.rows))}
                  <tr>
                    <td>{formatDateTime(timing.at)}</td>
                    <td>{timing.source}</td>
                    <td>{timing.rows}</td>
                    <td>{timing.total}</td>
                    <td class:slow={timing.tookMs > 50}>{timing.tookMs.toFixed(1)}ms</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>

        <div class="diag-block">
          <div class="diag-head">最近错误<span class="dim">最多保留 30 条</span></div>
          {#if recentErrors.length === 0}
            <div class="dim">没有错误</div>
          {:else}
            <ul class="error-list">
              {#each recentErrors as item (item.at + item.message)}
                <li><span class="dim">{formatDateTime(item.at)}</span> {item.message}</li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else}
        <div class="dim">正在读取…</div>
      {/if}
    {/if}
  </div>
</div>
