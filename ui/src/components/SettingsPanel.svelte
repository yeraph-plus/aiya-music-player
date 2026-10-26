<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { AppSettings, AmbientScheme, ListDensity, SpectrumStyle, ThemeMode } from '../lib/settings';
  import { AMBIENT_SCHEMES, SPECTRUM_STYLES } from '../lib/settings';
  import {
    SHORTCUT_ACTIONS,
    SHORTCUT_LABELS,
    findConflicts,
    formatBinding,
    resolveShortcutBindings,
    type ShortcutAction,
  } from '../lib/shortcuts';
  import { bindingFromEvent } from '../lib/keys';
  import Icon from './Icon.svelte';
  import Button from './ui/Button.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import Switch from './ui/Switch.svelte';
  import Header from './content/Header.svelte';
  import { capture } from '../stores/capture.svelte';

  interface Props {
    settings: AppSettings;
    onsave: (next: AppSettings) => void;
  }

  let { settings, onsave }: Props = $props();

  /**
   * 设置页 = 左分组导航 + 右内容（docs/24 B6）。
   *
   * **所有区块始终渲染**：扫描/beets 任务必须挂在常驻区块里，
   * 条件切换会让切走的那一侧任务进度直接卸载。左栏只是滚动定位 + 高亮，
   * 不是路由——高亮来自 scroll-spy（滚到哪个区块就亮哪组），
   * 与 LibraryView 的「路由级 tab」是两种东西，别复用 Toolbar。
   *
   * docs/29 批 A：「音乐库与任务」「诊断」两分区撤出设置页，升为侧栏一级
   * 「扫描入库」页（任务长时运行，需要可停留的一级路由）。这里只剩
   * 外观/播放/系统/快捷键四组纯配置。
   */
  type Section = 'appearance' | 'playback' | 'system' | 'shortcuts';
  let active = $state<Section>('appearance');
  let root = $state<HTMLElement | null>(null);

  const SECTIONS: Array<{ id: Section; label: string }> = [
    { id: 'appearance', label: '外观' },
    { id: 'playback', label: '播放' },
    { id: 'system', label: '系统' },
    { id: 'shortcuts', label: '快捷键' },
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
  // 快捷键区的即时冲突提示（诊断块的"快捷键冲突"已随分区搬去扫描入库页，
  // 但录制的当下就该看到冲突，这里保留一份轻量提示）
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
            <div class="setting-title">背景方案</div>
            <div class="setting-hint">固定渐变背景与强调色成套切换，不跟随封面</div>
          </div>
          <SegmentedControl
            label="背景方案"
            value={settings.ambient}
            options={AMBIENT_SCHEMES}
            onchange={(value) => apply({ ambient: value as AmbientScheme })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">频谱可视化</div>
            <div class="setting-hint">
              背景环境频谱总开关：开启后整个应用背景有随音乐起伏的环境光；关闭时引擎 tap 停止、零开销
            </div>
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
              「正在播放」页居中可视化的形态，切换即时生效；关闭 = 本页不画、只剩背景环境光
            </div>
          </div>
          <SegmentedControl
            label="频谱风格"
            value={settings.spectrumStyle}
            options={SPECTRUM_STYLES}
            onchange={(value) => apply({ spectrumStyle: value as SpectrumStyle })}
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

    </div><!-- /settings-main -->
  </div><!-- /settings-body -->
</div>

<style>
  /* docs/19 §5：提示文案是用户可能要复制的内容，从全局 user-select:none
     里豁免（诊断块的豁免随分区搬去了 ScanToolsView） */
  .setting-hint,
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
