<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { BeetsStatus, Diagnostics } from '../lib/ipc';
  import type { AppSettings, ListDensity, ThemeMode } from '../lib/settings';
  import {
    SHORTCUT_ACTIONS,
    SHORTCUT_LABELS,
    findConflicts,
    formatBinding,
    resolveShortcutBindings,
    type ShortcutAction,
  } from '../lib/shortcuts';
  import { bindingFromEvent } from '../lib/keys';
  import { formatBytes, formatDateTime, formatDurationLong, shortenPath } from '../lib/format';
  import Icon from './Icon.svelte';
  import LibraryTools from './LibraryTools.svelte';
  import Button from './ui/Button.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import Switch from './ui/Switch.svelte';
  import Header from './content/Header.svelte';
  import { capture } from '../stores/capture.svelte';

  interface Props {
    settings: AppSettings;
    diagnostics: Diagnostics | null;
    beets: BeetsStatus | null;
    onsave: (next: AppSettings) => void;
    onrefresh: () => void;
    onclearhistory: () => void;
    onopendatadir: () => void;
  }

  let { settings, diagnostics, beets, onsave, onrefresh, onclearhistory, onopendatadir }: Props =
    $props();

  const listTimings = $derived(diagnostics?.listTimings ?? []);
  const recentErrors = $derived(diagnostics?.recentErrors ?? []);

  /**
   * 设置页 = 左分组导航 + 右内容（docs/24 B6）。
   *
   * **所有区块始终渲染**：扫描/beets 任务必须挂在常驻区块里，
   * 条件切换会让切走的那一侧任务进度直接卸载。左栏只是滚动定位 + 高亮，
   * 不是路由——高亮来自 scroll-spy（滚到哪个区块就亮哪组），
   * 与 LibraryView 的「路由级 tab」是两种东西，别复用 Toolbar。
   */
  type Section = 'library' | 'appearance' | 'playback' | 'system' | 'shortcuts' | 'diagnostics';
  let active = $state<Section>('library');
  let root = $state<HTMLElement | null>(null);

  const SECTIONS: Array<{ id: Section; label: string }> = [
    { id: 'library', label: '音乐库与任务' },
    { id: 'appearance', label: '外观' },
    { id: 'playback', label: '播放' },
    { id: 'system', label: '系统' },
    { id: 'shortcuts', label: '快捷键' },
    { id: 'diagnostics', label: '诊断' },
  ];

  /** scroll-spy：滚动容器里找最后一个顶边已越过视口上沿 1/3 的区块 */
  function onScroll(): void {
    if (!root) return;
    const line = root.clientHeight / 3;
    let current: Section = SECTIONS[0].id;
    for (const { id } of SECTIONS) {
      const el = root.querySelector<HTMLElement>(`#settings-${id}`);
      if (el && el.getBoundingClientRect().top - root.getBoundingClientRect().top <= line) {
        current = id;
      }
    }
    active = current;
  }

  function jumpTo(id: Section): void {
    active = id;
    root?.querySelector(`#settings-${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  /**
   * 录制态**不放在组件里**：全局快捷键的 listener 也挂在 window 上，
   * 组件内 stopPropagation 拦不住它（同节点按注册顺序触发）。
   * 状态进 `stores/capture`，`lib/keyboard.ts` 自己看这个标志让路。
   */
  const capturing = $derived(capture.action);
  onDestroy(() => capture.stop());

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

  /** 录制快捷键时接管按键；Esc 只取消录制，不再有关闭面板语义（设置已是页面） */
  function onWindowKey(event: KeyboardEvent): void {
    const action = capture.action;
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      capture.stop();
      return;
    }
    if (event.key === 'Backspace' || event.key === 'Delete') {
      setShortcut(action, '');
      capture.stop();
      return;
    }
    const binding = bindingFromEvent(event);
    if (!binding) return;
    setShortcut(action, binding);
    capture.stop();
  }

  /**
   * 输出选项的取值**必须**与 `aiya-audio` 的 `ModePref::parse` /
   * `ClockPref::parse` 对得上（`crates/aiya-audio/src/engine.rs`）。
   * 后端对未知值一律按 `shared` / `auto` 处理——写错不会报错，只会静默失效，
   * 所以这里刻意只列后端认的字符串，不给"看起来更自然"的别名。
   */
  const OUTPUT_MODES: Array<{ value: string; label: string; hint: string }> = [
    { value: 'shared', label: '共享', hint: '与系统混音，其他程序照常出声' },
    { value: 'exclusive', label: '独占', hint: '优先独占，设备拒绝时自动回落共享' },
    { value: 'exclusiveRequired', label: '独占(强)', hint: '必须独占，设备拒绝就报错——不静默降级' },
    { value: 'null', label: '静音', hint: '不开音频设备：进度照常走但不出声（排查用）' },
  ];

  /** 分段控件要 `title`，这里把 `hint` 映射过去；模块级只算一次，
   *  否则每次渲染都造新数组，RadioGroup 会重挂、键盘焦点丢失。 */
  const OUTPUT_OPTIONS = OUTPUT_MODES.map(({ value, label, hint }) => ({ value, label, title: hint }));

  /**
   * 0 = 交给引擎按模式取默认（共享 200ms、独占按设备周期）
   *
   * value 写成字符串：分段控件的值是 `string`，这里若留 number 就要在
   * 每次渲染时 `String()` 一遍，反而更容易写错比较（`'0' === 0` 恒 false）。
   */
  const BUFFER_OPTIONS: Array<{ value: string; label: string }> = [
    { value: '0', label: '自动' },
    { value: '50', label: '50ms' },
    { value: '100', label: '100ms' },
    { value: '200', label: '200ms' },
  ];

  const CLOCK_OPTIONS: Array<{ value: string; label: string }> = [
    { value: 'auto', label: '自动' },
    { value: 'events', label: '事件' },
    { value: 'polling', label: '轮询' },
  ];

  /** 分段控件的选项一律写成模块级常量：内联数组字面量每次渲染都是新引用，
   *  RadioGroup 会连同 `title` 一起重挂，键盘焦点在重挂瞬间丢掉。 */
  const THEME_OPTIONS: Array<{ value: string; label: string }> = [
    { value: 'dark', label: '深色' },
    { value: 'light', label: '浅色' },
  ];

  const DENSITY_OPTIONS: Array<{ value: string; label: string }> = [
    { value: 'comfortable', label: '标准' },
    { value: 'compact', label: '紧凑' },
  ];

  const SPECTRUM_ENGINE_OPTIONS: Array<{ value: string; label: string }> = [
    { value: 'pro', label: '专业' },
    { value: 'compact', label: '简洁' },
  ];
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="page-scroll settings-page" bind:this={root} onscroll={onScroll}>
  <Header kicker="PREFERENCES" title="设置" />

  <div class="settings-body">
    <!-- 左分组导航（B6）：sticky 跟随滚动；高亮由 scroll-spy 写回，
         点击只是 scrollIntoView，不做条件切换（理由见组件头注释）。
         aria-current 让读屏器知道"现在在哪一组"，与视觉高亮同源。 -->
    <nav class="settings-rail" aria-label="设置分组">
      {#each SECTIONS as section (section.id)}
        <button
          class="rail-item"
          class:on={active === section.id}
          aria-current={active === section.id ? 'true' : undefined}
          onclick={() => jumpTo(section.id)}
        >
          {section.label}
        </button>
      {/each}
    </nav>

    <div class="settings-main">
      <section class="settings-section" id="settings-library">
        <div class="section-heading">
          <div><h2>音乐库与任务</h2></div>
        </div>
        <LibraryTools />
      </section>

      <section class="settings-section" id="settings-appearance">
        <div class="section-heading"><div><h2>外观</h2></div></div>
        <div class="setting-row">
          <div>
            <div class="setting-title">主题</div>
            <div class="setting-hint">浅色主题适合白天与投影，切换即时生效</div>
          </div>
          <SegmentedControl
            label="主题"
            value={settings.theme}
            options={THEME_OPTIONS}
            onchange={(value) => apply({ theme: value as ThemeMode })}
          >
            {#snippet icon(value)}
              <Icon name={value === 'dark' ? 'moon' : 'sun'} size={13} />
            {/snippet}
          </SegmentedControl>
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">列表密度</div>
            <div class="setting-hint">紧凑模式一屏多显示约 20%，适合大曲库浏览</div>
          </div>
          <SegmentedControl
            label="列表密度"
            value={settings.density}
            options={DENSITY_OPTIONS}
            onchange={(value) => apply({ density: value as ListDensity })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">封面主色</div>
            <div class="setting-hint">从封面提取强调色；关掉则使用固定蓝色</div>
          </div>
          <Switch
            label="封面主色"
            checked={settings.accentFromArtwork}
            onchange={(checked) => apply({ accentFromArtwork: checked })}
          />
        </div>
      </section>

      <section class="settings-section" id="settings-playback">
        <div class="section-heading"><div><h2>播放</h2></div></div>
        <div class="setting-row">
          <div>
            <div class="setting-title">输出模式</div>
            <div class="setting-hint">
              {OUTPUT_MODES.find((m) => m.value === settings.output.mode)?.hint ??
                '共享：与系统混音，其他程序照常出声'}
            </div>
          </div>
          <SegmentedControl
            label="输出模式"
            value={settings.output.mode}
            options={OUTPUT_MODES}
            onchange={(value) => apply({ output: { ...settings.output, mode: value } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">缓冲</div>
            <div class="setting-hint">
              越深越抗抖、延迟越大；「自动」按模式取默认（共享 200ms，独占按设备周期）
            </div>
          </div>
          <SegmentedControl
            label="缓冲"
            value={String(settings.output.bufferMs)}
            options={BUFFER_OPTIONS}
            onchange={(value) => apply({ output: { ...settings.output, bufferMs: Number(value) } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">时钟</div>
            <div class="setting-hint">
              自动 = 独占先试事件、失败再试轮询；共享固定用事件（更省电、延迟更稳）
            </div>
          </div>
          <SegmentedControl
            label="时钟"
            value={settings.output.clock}
            options={CLOCK_OPTIONS}
            onchange={(value) => apply({ output: { ...settings.output, clock: value } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">暂停时释放设备</div>
            <div class="setting-hint">
              暂停就关流，让独占设备回到系统；再播放会重新打开（有几十毫秒空白）
            </div>
          </div>
          <Switch
            label="暂停时释放设备"
            checked={settings.output.releaseOnPause}
            onchange={(checked) =>
              apply({ output: { ...settings.output, releaseOnPause: checked } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">启动时自动播放</div>
            <div class="setting-hint">打开应用就接着放上次那一首；关闭则停在暂停状态</div>
          </div>
          <Switch
            label="启动时自动播放"
            checked={settings.autoplayOnStart}
            onchange={(checked) => apply({ autoplayOnStart: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">记住播放进度</div>
            <div class="setting-hint">下次启动从上次停下的位置继续；关闭则每次从头开始</div>
          </div>
          <Switch
            label="记住播放进度"
            checked={settings.rememberPosition}
            onchange={(checked) => apply({ rememberPosition: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">播放时阻止系统休眠</div>
            <div class="setting-hint">只阻止系统睡眠，屏幕仍会按系统设置熄掉</div>
          </div>
          <Switch
            label="播放时阻止系统休眠"
            checked={settings.preventSleep}
            onchange={(checked) => apply({ preventSleep: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">音量归一化</div>
            <div class="setting-hint">
              按 ReplayGain 标签换算音量系数。近似实现：不改变你设置的音量，只做播放时的补偿
            </div>
          </div>
          <Switch
            label="音量归一化"
            checked={settings.normalizeVolume}
            onchange={(checked) => apply({ normalizeVolume: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">频谱可视化</div>
            <div class="setting-hint">在「正在播放」页显示实时频谱（ADR-037：开启后启用渲染层音频图）</div>
          </div>
          <Switch
            label="频谱可视化"
            checked={settings.spectrum}
            onchange={(checked) => apply({ spectrum: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">频谱风格</div>
            <div class="setting-hint">
              专业：五形态渲染（渐变条 / LED / 镜像 / 径向 / 反射）；简洁：自研轻量条形。
              两种风格数据同源（48 分带实时频谱），差别只在渲染层。
            </div>
          </div>
          <SegmentedControl
            label="频谱风格"
            value={settings.spectrumEngine}
            options={SPECTRUM_ENGINE_OPTIONS}
            onchange={(value) => apply({ spectrumEngine: value as 'compact' | 'pro' })}
          />
        </div>
      </section>

      <section class="settings-section" id="settings-system">
        <div class="section-heading"><div><h2>系统</h2></div></div>
        <div class="setting-row">
          <div>
            <div class="setting-title">关闭窗口时最小化到托盘</div>
            <div class="setting-hint">
              打开后点关闭只是隐藏窗口，播放继续；要退出请用托盘菜单或 Ctrl+Q
            </div>
          </div>
          <Switch
            label="关闭窗口时最小化到托盘"
            checked={settings.closeToTray}
            onchange={(checked) => apply({ closeToTray: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">系统媒体键</div>
            <div class="setting-hint">
              播放器的标题、封面与播放状态同步到系统媒体面板，媒体键由系统路由，失焦也生效
            </div>
          </div>
          <Switch
            label="系统媒体键"
            checked={settings.mediaKeys}
            onchange={(checked) => apply({ mediaKeys: checked })}
          />
        </div>
      </section>

      <section class="settings-section" id="settings-shortcuts">
        <div class="section-heading"><div><h2>快捷键</h2></div></div>
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
                onclick={() => capture.start(action)}
              >
                {capturing === action ? '按下新键…' : formatBinding(bindings[action])}
              </button>
            </div>
          {/each}
        </div>
        <div class="setting-foot">
          <span class="dim">Esc 取消录制，Backspace 清空绑定。↑/↓ 与 Enter 是列表导航，不占用快捷键。</span>
          <Button size="sm" onclick={() => apply({ shortcuts: {} })}>恢复默认</Button>
        </div>
      </section>

      <section class="settings-section" id="settings-diagnostics">
        <div class="section-heading"><div><h2>诊断</h2></div></div>
        <div class="diag-actions">
          <Button size="sm" onclick={onrefresh}>
            {#snippet icon()}<Icon name="refresh" size={13} />{/snippet}
            刷新
          </Button>
          <Button size="sm" onclick={onopendatadir}>打开数据目录</Button>
          <Button variant="danger" size="sm" onclick={onclearhistory}>清空播放历史</Button>
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
                <thead><tr><th>时间</th><th>来源</th><th>行数</th><th>总数</th><th>命令耗时</th><th>SQL</th></tr></thead>
                <tbody>
                  {#each listTimings.slice(-8).reverse() as timing}
                    <tr>
                      <td>{formatDateTime(timing.atMs)}</td>
                      <td>{timing.source}</td>
                      <td>{timing.rows}</td>
                      <td>{timing.total}</td>
                      <td class:slow={timing.tookMs > 50}>{timing.tookMs.toFixed(1)}ms</td>
                      <td class:slow={timing.dbTookMs > 50}>{timing.dbTookMs.toFixed(1)}ms</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>

          <div class="diag-block">
            <div class="diag-head">
              快捷键冲突
              {#if conflicts.length === 0}
                <span class="dim">无</span>
              {:else}
                <span class="danger">{conflicts.length} 组</span>
              {/if}
            </div>
            {#if conflicts.length === 0}
              <div class="dim">同一组按键绑了多个动作时会出现在这里，到「快捷键」区块可以改。</div>
            {:else}
              <ul class="error-list">
                {#each conflicts as conflict (conflict.binding)}
                  <li>
                    <b>{formatBinding(conflict.binding)}</b>
                    {conflict.actions.map((action) => SHORTCUT_LABELS[action as ShortcutAction]).join('、')}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>

          <div class="diag-block">
            <div class="diag-head">最近错误<span class="dim">最多保留 30 条，新的在上</span></div>
            {#if recentErrors.length === 0}
              <div class="dim">没有错误</div>
            {:else}
              <ul class="error-list">
                {#each recentErrors.slice().reverse() as item}
                  <li>
                    <span class="dim">{formatDateTime(item.atMs)}</span>
                    <span class="danger">{item.code}</span>
                    {item.message}
                    {#if item.detail}
                      <span class="dim" title={item.detail}>（有详情）</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {:else}
          <div class="dim">正在读取…</div>
        {/if}
      </section>
    </div><!-- /settings-main -->
  </div><!-- /settings-body -->
</div>

<style>
  /* docs/19 §5：诊断值/路径/beets 提示/快捷键冲突说明是用户可能要复制的内容，
     从全局 user-select:none 里豁免 */
  .setting-hint,
  .diag-grid,
  .diag-block .dim,
  .error-list,
  .setting-foot .dim {
    user-select: text;
    cursor: text;
  }

  /* 左分组导航 + 右内容（B6）。外层 .page-scroll 仍是唯一滚动容器，
     左栏 sticky 跟随；右列 max-width 收窄到原 860px 内 */
  .settings-body {
    display: grid;
    grid-template-columns: 148px minmax(0, 1fr);
    gap: 22px;
    align-items: start;
    max-width: 1020px;
  }

  .settings-rail {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .rail-item {
    padding: 6px 10px;
    border-radius: var(--radius-md);
    text-align: left;
    font-size: 12.5px;
    color: var(--text-2);
    border-left: 2px solid transparent;
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
  }

  .rail-item:hover {
    color: var(--text-1);
    background: var(--surface-hover);
  }

  .rail-item.on {
    color: var(--text-1);
    background: var(--surface-hover);
    border-left-color: var(--accent);
    font-weight: 600;
  }

  /* 窄窗口退回单列：左栏变成顶部横向条 */
  @media (max-width: 720px) {
    .settings-body {
      grid-template-columns: 1fr;
    }

    .settings-rail {
      position: static;
      flex-direction: row;
      flex-wrap: wrap;
    }

    .rail-item {
      border-left: none;
      border-radius: var(--radius-md);
    }

    .rail-item.on {
      border-left: none;
      outline: 1px solid var(--accent);
    }
  }
</style>
