<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { t, type Msg } from '../lib/i18n.svelte';
  import { Clock3, Database, Disc3, ExternalLink, HardDrive, ListMusic, Mic2, TriangleAlert } from '@lucide/svelte';
  import type {
    AppSettings,
    AmbientScheme,
    BgSpectrumStyle,
    RidgeGlow,
    SpectrumStyle,
    ThemeMode,
  } from '../lib/settings';
  import {
    AMBIENT_SCHEMES,
    BG_SPECTRUM_STYLES,
    RIDGE_GLOWS,
    SPECTRUM_STYLES,
    THEME_MODES,
    UI_SCALE_OPTIONS,
    normalizeUiScale,
  } from '../lib/settings';
  import StatBlock from './ui/StatBlock.svelte';
import TextTabs from './ui/TextTabs.svelte';
  import {
    SHORTCUT_ACTIONS,
    SHORTCUT_LABELS,
    findConflicts,
    formatBinding,
    resolveShortcutBindings,
    type ShortcutAction,
  } from '../lib/shortcuts';
  import { bindingFromEvent } from '../lib/keys';
  import { detectCapability } from '../lib/render/capability';
  import { formatBytes, formatDateTime, formatDurationLong } from '../lib/format';
  import afdianQr from '../assets/afdian-qr.png';
  import Icon from './Icon.svelte';
  import LibraryTools from './LibraryTools.svelte';
  import Button from './ui/Button.svelte';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import Switch from './ui/Switch.svelte';
  import Header from './content/Header.svelte';
  import { capture } from '../stores/capture.svelte';
  import { library } from '../stores/library.svelte';
  import { ui, type SettingsSection } from '../stores/ui.svelte';

  interface Props {
    settings: AppSettings;
    onsave: (next: AppSettings) => void;
  }

  let { settings, onsave }: Props = $props();

  /**
   * 分段控件选项的 label 在 `lib/settings.ts` 存 i18n 键（设置项清单是该控件的
   * 真源，不持有具体文案）；控件吃 string，在此统一收口成 `t()`。
   */
  function segOptions<V extends string>(
    options: ReadonlyArray<{ value: V; label: Msg }>,
  ): Array<{ value: V; label: string }> {
    return options.map((option) => ({ value: option.value, label: t(option.label) }));
  }

  /**
   * 识别信息与排障数据：版本、平台、Schema 版本、数据目录、最近错误。
   *
   * 描述对象是整个应用而非某个功能，因此归入「关于」组。数据源为
   * `ui.diagnostics`，与「系统」组的集成状态共用同一份快照。
   */
  const sys = $derived(ui.diagnostics);

  /** 关于段的出站链接（宿主 app_open_url 白名单内的两个地址）。 */
  const REPO_URL = 'https://github.com/yeraph-plus/aiya-music-player';
  const RELEASES_URL = `${REPO_URL}/releases`;
  const AFDIAN_URL = 'https://afdian.com/a/NyaaACG';
  const recentErrors = $derived(sys?.recentErrors ?? []);

  /**
   * 曲库统计：曲目、专辑、艺人、总时长、索引大小、位置缺失。
   *
   * 只读派生值。拉取发生在 `boot.ts` 启动流程与每次扫描收尾
   * （`library.loadStats`）；本区块不发请求，避免每次进入设置页触发一次全库统计。
   */
  const stats = $derived(library.stats);

  /**
   * 布局契约：六个区块全部平铺渲染，顶部 tab 只做跳转定位。
   *
   * 高亮仅由点击写入，滚动不写回。若改由 scroll-spy 让滚动反向驱动高亮，
   * 平滑滚动期间程序写入的 scrollTop 会连续触发高亮更新，一次跳转产生数次
   * 可见抖动。tab 的语义是"跳到该组"而不是"当前位于该组"，因此不需要双向同步。
   *
   * 分组顺序按数据 → 内容 → 应用：曲库居首，目录、扫描、执行状态、统计等
   * 唯一带实际内容的区块都在该组，也是进入设置页的主要目的；其后为外观、
   * 播放等偏好，系统、快捷键、关于收尾。
   */
  type Section = SettingsSection;
  let active = $state<Section>('library');

  const SECTIONS: Array<{ id: Section; labelKey: Msg }> = [
    { id: 'library', labelKey: 'settings.section.library' },
    { id: 'appearance', labelKey: 'settings.section.appearance' },
    { id: 'playback', labelKey: 'settings.section.playback' },
    { id: 'system', labelKey: 'settings.section.system' },
    { id: 'shortcuts', labelKey: 'settings.section.shortcuts' },
    { id: 'about', labelKey: 'settings.section.about' },
  ];

  /** 各区块根元素（bind:this 填充），tab 跳转的目标 */
  const sectionEls: Record<Section, HTMLElement | null> = {
    library: null,
    appearance: null,
    playback: null,
    system: null,
    shortcuts: null,
    about: null,
  };

  function jumpTo(section: Section): void {
    active = section;
    sectionEls[section]?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  /**
   * 一次性跳转请求（`nav.settingsAt()` 写入，本组件消费后立即清空）。
   *
   * 需要 `await tick()`：请求在路由切换之前写入，本组件可能在本帧才首次挂载，
   * 此时 `sectionEls` 尚未由 `bind:this` 填充，直接 `scrollIntoView` 会被可选链
   * 静默跳过。tick 之后绑定已完成。
   *
   * 清空而非保留：这是一次性请求，不是"当前分组"状态；后者会让滚动与高亮
   * 重新互相驱动，与本页的布局契约冲突。
   */
  $effect(() => {
    const want = ui.settingsSection;
    if (!want) return;
    ui.settingsSection = null;
    void tick().then(() => jumpTo(want));
  });

  /**
   * 录制态不保存在组件内：全局快捷键的 listener 同样挂在 window 上，
   * 组件内的 `stopPropagation` 无法拦截（同一节点按注册顺序触发）。
   * 状态存放于 `stores/capture`，`lib/keyboard.ts` 读取该标志后让路。
   */
  const capturing = $derived(capture.action);
  onDestroy(() => capture.stop());

  const bindings = $derived(resolveShortcutBindings(settings.shortcuts));
  // 快捷键区的即时冲突提示：诊断块的「快捷键冲突」留在系统组，本处保留一份
  // 轻量提示，使录制当下即可看到冲突。
  const conflicts = $derived(findConflicts(bindings));

  function apply(patch: Partial<AppSettings>): void {
    onsave({ ...settings, ...patch });
  }

  /**
   * 全局 3D 闸门：关闭后 `bgSpectrum` / `npSpectrum` 的取值仍保留
   * 在设置中，但不参与渲染。相关行置灰而非隐藏：用户需要看见被关掉的是哪几项，
   * 隐藏会使这些设置看起来已被移除。
   */
  const gl = $derived(settings.glEffects);

  /**
   * 本机 3D 能力（探测结果在 `lib/render/capability` 内缓存，每次挂载只探一次）。
   *
   * `glEffects` 的后果对用户不可见，因此由应用直接给出结论而不是让用户试错：
   * 软件渲染（远程桌面、禁用硬件加速）下保持开启只会增加开销，提示语据此
   * 建议关闭。
   */
  const cap = detectCapability();

  function setShortcut(action: ShortcutAction, binding: string): void {
    const shortcuts = { ...settings.shortcuts };
    if (!binding) delete shortcuts[action];
    else shortcuts[action] = binding;
    apply({ shortcuts });
  }

  /** 录制快捷键时接管按键。Esc 只取消录制：设置已是独立页面，不存在关闭面板语义 */
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
   * 输出模式的取值须与 `aiya-audio` 的 `ModePref::parse` / `ClockPref::parse`
   * 一致（`crates/aiya-audio/src/engine.rs`）。后端对未知取值按 `shared` / `auto`
   * 处理，写错不报错而是静默降级，因此此处只列后端识别的字符串，不提供别名。
   *
   * 后端的 `'null'`（静音）排查档**刻意不列**：静音只有播放条一个入口，设置页
   * 再给一份是双份实现（2026-11-05 拍板）。若存量配置停留在该档，hint 走
   * `settings.output.hint.fallback` 兜底，用户随手选一档即写回合法值。
   */
  const OUTPUT_MODES: Array<{ value: string; label: string; hint: string }> = $derived([
    { value: 'shared', label: t('settings.output.mode.shared'), hint: t('settings.output.mode.shared.hint') },
    {
      value: 'exclusive',
      label: t('settings.output.mode.exclusive'),
      hint: t('settings.output.mode.exclusive.hint'),
    },
    {
      value: 'exclusiveRequired',
      label: t('settings.output.mode.exclusiveRequired'),
      hint: t('settings.output.mode.exclusiveRequired.hint'),
    },
  ]);

  /** 分段控件使用 `title`，此处把 `hint` 映射过去。$derived 仅在语言变化时
   *  重算（t() 依赖 locale），避免每次渲染新数组引用导致 RadioGroup 重挂。 */
  const OUTPUT_OPTIONS = $derived(
    OUTPUT_MODES.map(({ value, label, hint }) => ({ value, label, title: hint })),
  );

  /**
   * 0 = 交由引擎按输出模式取默认值（共享 200ms，独占取设备周期）。
   *
   * 值写成字符串：分段控件的值为 `string`，保留 number 会导致每次渲染都要
   * `String()` 转换，且 `'0' === 0` 恒为 false，比较更容易出错。
   */
  const BUFFER_OPTIONS: Array<{ value: string; label: string }> = $derived([
    { value: '0', label: t('settings.playback.buffer.auto') },
    { value: '50', label: '50ms' },
    { value: '100', label: '100ms' },
    { value: '200', label: '200ms' },
  ]);

  /**
   * 自动刷新间隔（秒）。0 = 仅保留"切回窗口时刷新"。
   *
   * 上限取 5 分钟：更长的间隔不如关闭定时（切回窗口刷新已覆盖主要场景），
   * 保留半小时级档位反而会让用户误以为定时刷新失效。
   */
  const REFRESH_OPTIONS: Array<{ value: string; label: string }> = $derived([
    { value: '0', label: t('settings.system.refresh.off') },
    { value: '30', label: t('settings.system.refresh.30s') },
    { value: '120', label: t('settings.system.refresh.2m') },
    { value: '300', label: t('settings.system.refresh.5m') },
  ]);

  /** 播放记录保留周期（0 = 永久）；宿主据此裁剪 history 行 */
  const HISTORY_DAYS_OPTIONS: Array<{ value: string; label: string }> = $derived([
    { value: '30', label: t('settings.system.history.30d') },
    { value: '90', label: t('settings.system.history.90d') },
    { value: '180', label: t('settings.system.history.halfYear') },
    { value: '365', label: t('settings.system.history.year') },
    { value: '0', label: t('settings.system.history.forever') },
  ]);

  const CLOCK_OPTIONS: Array<{ value: string; label: string }> = $derived([
    { value: 'auto', label: t('settings.playback.clock.auto') },
    { value: 'events', label: t('settings.playback.clock.events') },
    { value: 'polling', label: t('settings.playback.clock.polling') },
  ]);

  /** 分段控件的选项一律写成模块级常量：内联数组字面量每次渲染都是新引用，
   *  RadioGroup 会连同 `title` 一起重挂，键盘焦点在重挂瞬间丢掉。主题清单
   *  （THEME_MODES）与其他档位一样以 settings.ts 为真源，这里只消费。 */
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="settings-page">
  <!-- 头部与内容共用一条居中列：本页为表单式布局，内容列上限 1020，
       头部若单独贴屏幕左缘会与内容列错位 -->
  <div class="settings-headwrap">
    <div class="page-pad">
      <!-- 大标题固定「设置」，右侧 tab 只做跳转定位 -->
      <Header kicker="SETTINGS" title={t('settings.title')} subtitle={t('settings.subtitle')}>
        {#snippet actions()}
          <!-- 语义用 nav + aria-current，而非 role="tablist"/"tab"（同 content/Toolbar）：tab 只执行 scrollIntoView
          跳转，六个区块平铺常驻，不构成"切换面板"；套用 tab 角色会让读屏软件等待一个不存在的 tabpanel。语义与外观统一由 TextTabs 提供。 -->
          <TextTabs
            items={SECTIONS.map((s) => ({ id: s.id, label: t(s.labelKey) }))}
            value={active}
            onchange={(id) => jumpTo(id as (typeof SECTIONS)[number]['id'])}
            label={t('settings.nav.aria')}
          />
        {/snippet}
      </Header>
    </div>
  </div>

  <!-- 内容容器独立滚动，头部（标题 + tab）固定。六个区块全部平铺渲染，
       tab 点击只执行 scrollIntoView。分组顺序：曲库居首，其余按内容 → 应用。 -->
  <div class="settings-scroll">
    <!--
      曲库组。

      「扫描入库」独立页撤销的依据：beets 拆为独立「整理库」页之后
      该页只剩导入曲库目录一件事，占用一个一级导航席位与一张卡片的体量不匹配。
      曲库目录属于配置项（与"播放记录保留多久"同性质），归设置页。

      排在首位：六组中只有该组含实际内容（目录列表、扫描、执行状态、统计），
      其余五组均为偏好开关。

      与已撤销页面的差别仅在排版：使用设置页统一的 group-title，不再有页面级
      大标题与组内小标题；统计卡与目录卡原样引入（`components/LibraryTools`
      不依赖页面壳）。
    -->
    <section class="settings-section" id="settings-library" bind:this={sectionEls.library}>
      <h3 class="group-title">{t('settings.section.library')}</h3>

      <!-- 统计卡：图标 + 数值 + 标签。数值使用等宽数字（tabular-nums）以免更新时抖动；图标用中性描边样式，不与数值争夺视觉权重。六项铺满 3×2
      网格。索引大小与文件容量是两个量：前者为 SQLite 索引自身体积，后者为音频文件总量 -->
      {#if stats}
        <div class="lib-stats">
          <StatBlock layout="row" value={stats.tracks} label={t('settings.library.stats.tracks')}>
            {#snippet icon()}<ListMusic size={17} />{/snippet}
          </StatBlock>
          <StatBlock layout="row" value={stats.albums} label={t('settings.library.stats.albums')}>
            {#snippet icon()}<Disc3 size={17} />{/snippet}
          </StatBlock>
          <StatBlock layout="row" value={stats.artists} label={t('settings.library.stats.artists')}>
            {#snippet icon()}<Mic2 size={17} />{/snippet}
          </StatBlock>
          <StatBlock
            layout="row"
            value={formatDurationLong(stats.totalDurationMs)}
            label={t('settings.library.stats.totalDuration')}
          >
            {#snippet icon()}<Clock3 size={17} />{/snippet}
          </StatBlock>
          <StatBlock
            layout="row"
            value={formatBytes(stats.totalBytes)}
            label={t('settings.library.stats.fileSize')}
          >
            {#snippet icon()}<HardDrive size={17} />{/snippet}
          </StatBlock>
          <StatBlock
            layout="row"
            value={formatBytes(stats.dbBytes)}
            label={t('settings.library.stats.indexSize')}
          >
            {#snippet icon()}<Database size={17} />{/snippet}
          </StatBlock>
          {#if stats.missingLocations > 0}
            <StatBlock
              layout="row"
              tone="warn"
              value={stats.missingLocations}
              label={t('settings.library.stats.missingLocations')}
            >
              {#snippet icon()}<TriangleAlert size={17} />{/snippet}
            </StatBlock>
          {/if}
        </div>
      {/if}

      <!-- 16px 出血：统计卡与目录卡自带 16px 水平内边距，与内容列（720 居中）叠加后卡内文字比下方设置行左缘多缩进
      16px，整块显得比页面窄一圈。负 margin 抵消该差值，卡内文字回到与设置行相同的左缘。 -->
      <div class="lib-bleed"><LibraryTools /></div>
    </section>
    <section class="settings-section" bind:this={sectionEls.appearance}>
      <h3 class="group-title">{t('settings.section.appearance')}</h3>
      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.appearance.theme')}</div>
            <div class="setting-hint">
              {settings.theme === 'system'
                ? t('settings.appearance.theme.hint.auto')
                : t('settings.appearance.theme.hint.light')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.theme')}
            value={settings.theme}
            options={segOptions(THEME_MODES)}
            onchange={(value) => apply({ theme: value as ThemeMode })}
          >
            {#snippet icon(value)}
              <Icon name={value === 'dark' ? 'moon' : value === 'light' ? 'sun' : 'monitor'} size={13} />
            {/snippet}
          </SegmentedControl>
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.language.title')}</div>
            <div class="setting-hint">{t('settings.language.hint')}</div>
          </div>
          <SegmentedControl
            label={t('settings.language.title')}
            value={settings.language === 'en' ? 'en' : 'zh'}
            options={[
              { value: 'zh', label: '中文' },
              { value: 'en', label: 'English' },
            ]}
            onchange={(value) => apply({ language: value })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.appearance.themeColor')}</div>
            <div class="setting-hint">{t('settings.appearance.themeColor.hint')}</div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.themeColor')}
            value={settings.ambient}
            options={segOptions(AMBIENT_SCHEMES)}
            onchange={(value) => apply({ ambient: value as AmbientScheme })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.appearance.uiScale')}</div>
            <div class="setting-hint">
              {t('settings.appearance.uiScale.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.uiScale')}
            value={String(settings.uiScale)}
            options={UI_SCALE_OPTIONS}
            onchange={(value) => apply({ uiScale: normalizeUiScale(value) })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.appearance.glEffects')}</div>
            <div class="setting-hint">
              {t('settings.appearance.glEffects.hint')}
              {#if !cap.hardware}
                <strong class="hint-warn">{t('settings.appearance.glEffects.warn')}</strong>
              {/if}
            </div>
          </div>
          <Switch
            label={t('settings.appearance.glEffects')}
            checked={settings.glEffects}
            onchange={(checked) => apply({ glEffects: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">{t('settings.appearance.bgSpectrum')}</div>
            <div class="setting-hint">
              {gl
                ? t('settings.appearance.bgSpectrum.hint.on')
                : t('settings.appearance.offByGl')}
            </div>
          </div>
          <Switch
            label={t('settings.appearance.bgSpectrum')}
            checked={settings.bgSpectrum}
            disabled={!gl}
            onchange={(checked) => apply({ bgSpectrum: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">{t('settings.appearance.bgSpectrumStyle')}</div>
            <div class="setting-hint">
              {gl
                ? t('settings.appearance.bgSpectrumStyle.hint.on')
                : t('settings.appearance.offByGl')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.bgSpectrumStyle')}
            value={settings.bgSpectrumStyle}
            options={segOptions(BG_SPECTRUM_STYLES)}
            disabled={!gl}
            onchange={(value) => apply({ bgSpectrumStyle: value as BgSpectrumStyle })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">{t('settings.appearance.ridgeGlow')}</div>
            <div class="setting-hint">
              {gl
                ? t('settings.appearance.ridgeGlow.hint.on')
                : t('settings.appearance.offByGl')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.ridgeGlow')}
            value={settings.ridgeGlow}
            options={segOptions(RIDGE_GLOWS)}
            disabled={!gl}
            onchange={(value) => apply({ ridgeGlow: value as RidgeGlow })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">{t('settings.appearance.npSpectrum')}</div>
            <div class="setting-hint">
              {gl
                ? t('settings.appearance.npSpectrum.hint.on')
                : t('settings.appearance.offByGl')}
            </div>
          </div>
          <Switch
            label={t('settings.appearance.npSpectrum')}
            checked={settings.npSpectrum}
            disabled={!gl}
            onchange={(checked) => apply({ npSpectrum: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">{t('settings.appearance.spectrumStyle')}</div>
            <div class="setting-hint">
              {t('settings.appearance.spectrumStyle.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.appearance.spectrumStyle')}
            value={settings.spectrumStyle}
            options={segOptions(SPECTRUM_STYLES)}
            disabled={!gl}
            onchange={(value) => apply({ spectrumStyle: value as SpectrumStyle })}
          />
        </div>
      </section>

    <section class="settings-section" bind:this={sectionEls.playback}>
      <h3 class="group-title">{t('settings.section.playback')}</h3>
      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.output.title')}</div>
            <div class="setting-hint">
              {OUTPUT_MODES.find((m) => m.value === settings.output.mode)?.hint ??
                t('settings.output.hint.fallback')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.output.title')}
            value={settings.output.mode}
            options={OUTPUT_MODES}
            onchange={(value) => apply({ output: { ...settings.output, mode: value } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.buffer')}</div>
            <div class="setting-hint">
              {t('settings.playback.buffer.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.playback.buffer')}
            value={String(settings.output.bufferMs)}
            options={BUFFER_OPTIONS}
            onchange={(value) => apply({ output: { ...settings.output, bufferMs: Number(value) } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.clock')}</div>
            <div class="setting-hint">
              {t('settings.playback.clock.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.playback.clock')}
            value={settings.output.clock}
            options={CLOCK_OPTIONS}
            onchange={(value) => apply({ output: { ...settings.output, clock: value } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.releaseOnPause')}</div>
            <div class="setting-hint">
              {t('settings.playback.releaseOnPause.hint')}
            </div>
          </div>
          <Switch
            label={t('settings.playback.releaseOnPause')}
            checked={settings.output.releaseOnPause}
            onchange={(checked) =>
              apply({ output: { ...settings.output, releaseOnPause: checked } })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.autoplayOnStart')}</div>
            <div class="setting-hint">{t('settings.playback.autoplayOnStart.hint')}</div>
          </div>
          <Switch
            label={t('settings.playback.autoplayOnStart')}
            checked={settings.autoplayOnStart}
            onchange={(checked) => apply({ autoplayOnStart: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.rememberPosition')}</div>
            <div class="setting-hint">{t('settings.playback.rememberPosition.hint')}</div>
          </div>
          <Switch
            label={t('settings.playback.rememberPosition')}
            checked={settings.rememberPosition}
            onchange={(checked) => apply({ rememberPosition: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.preventSleep')}</div>
            <div class="setting-hint">{t('settings.playback.preventSleep.hint')}</div>
          </div>
          <Switch
            label={t('settings.playback.preventSleep')}
            checked={settings.preventSleep}
            onchange={(checked) => apply({ preventSleep: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.playback.normalizeVolume')}</div>
            <div class="setting-hint">
              {t('settings.playback.normalizeVolume.hint')}
            </div>
          </div>
          <Switch
            label={t('settings.playback.normalizeVolume')}
            checked={settings.normalizeVolume}
            onchange={(checked) => apply({ normalizeVolume: checked })}
          />
        </div>
      </section>

    <section class="settings-section" bind:this={sectionEls.system}>
      <h3 class="group-title">{t('settings.section.system')}</h3>
      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.system.disableSystemScale')}</div>
            <div class="setting-hint">
              {t('settings.system.disableSystemScale.hint')}
            </div>
          </div>
          <Switch
            label={t('settings.system.disableSystemScale')}
            checked={settings.disableSystemScale}
            onchange={(checked) => apply({ disableSystemScale: checked })}
          />
        </div>

      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.system.closeToTray')}</div>
            <div class="setting-hint">
              {t('settings.system.closeToTray.hint')}
            </div>
          </div>
          <Switch
            label={t('settings.system.closeToTray')}
            checked={settings.closeToTray}
            onchange={(checked) => apply({ closeToTray: checked })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.system.autoRefresh')}</div>
            <div class="setting-hint">
              {t('settings.system.autoRefresh.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.system.autoRefresh.control')}
            value={String(settings.refreshSeconds ?? 30)}
            options={REFRESH_OPTIONS}
            onchange={(value) => apply({ refreshSeconds: Number(value) })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.system.historyRetention')}</div>
            <div class="setting-hint">
              {t('settings.system.historyRetention.hint')}
            </div>
          </div>
          <SegmentedControl
            label={t('settings.system.historyRetention.control')}
            value={String(settings.historyDays ?? 90)}
            options={HISTORY_DAYS_OPTIONS}
            onchange={(value) => apply({ historyDays: Number(value) })}
          />
        </div>
      </section>

    <section class="settings-section" bind:this={sectionEls.shortcuts}>
      <h3 class="group-title">{t('settings.section.shortcuts')}</h3>

      <!-- 录制规则与恢复默认放在组首：用户在点击绑定键之前需要知道 Esc / Backspace 的行为。外框使其读作操作须知，而非列表末尾的附注。 -->
      <div class="shortcut-foot">
        <span class="dim">{t('settings.shortcuts.footHint')}</span>
        <Button size="sm" onclick={() => apply({ shortcuts: {} })}>{t('settings.shortcuts.reset')}</Button>
      </div>

      {#if conflicts.length > 0}
          <div class="notice danger">
            {#each conflicts as conflict (conflict.binding)}
              <div>
                {t('settings.shortcuts.conflict', {
                  binding: formatBinding(conflict.binding),
                  actions: conflict.actions
                    .map((action) => SHORTCUT_LABELS[action as ShortcutAction])
                    .join(t('settings.shortcuts.actionSep')),
                })}
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
                {capturing === action ? t('settings.shortcuts.pressNewKey') : formatBinding(bindings[action])}
              </button>
            </div>
          {/each}
        </div>
      </section>

    <!-- 关于组：识别信息与排障。布局与设置项一致（一行一项）。平台、Schema 版本、播放列表计数不展示：属内部信息或恒为
    0。不提供「重新读取」按钮：诊断随页面加载自动拉取。「获取更新」为占位按钮，功能未提供，恒禁用。数据目录与「打开目录」合为一行。 -->
    <section class="settings-section settings-about" bind:this={sectionEls.about}>
      <h3 class="group-title">{t('settings.section.about')}</h3>
      <div class="setting-row">
        <div>
          <div class="setting-title">{t('settings.about.version')}</div>
          <div class="setting-hint">{sys?.version ?? '…'}</div>
        </div>
        <div class="about-actions">
          <Button size="sm" onclick={() => ui.openUrl(RELEASES_URL)}
            title={t('settings.about.releases.open')}>
            {#snippet icon()}<ExternalLink size={14} />{/snippet}
          </Button>
          <Button size="sm" disabled title={t('settings.about.checkUpdates.title')}>
            {t('settings.about.checkUpdates')}
          </Button>
        </div>
      </div>

      <div class="setting-row">
        <div class="datadir-cell">
          <div class="setting-title">{t('settings.about.dataDir')}</div>
          <!-- 完整路径可复制（user-select 豁免见样式区），过长省略、悬停给全路径 -->
          <div class="setting-hint datadir-path" title={sys?.dataDir ?? ''}>{sys?.dataDir ?? '…'}</div>
        </div>
        <Button size="sm" onclick={() => ui.openDataDir()}>{t('settings.about.openDir')}</Button>
      </div>

      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.about.repo')}</div>
            <div class="setting-hint datadir-path" title={REPO_URL}>{REPO_URL.replace('https://', '')}</div>
          </div>
          <Button size="sm" onclick={() => ui.openUrl(REPO_URL)}>
            {#snippet icon()}<ExternalLink size={14} />{/snippet}
            {t('settings.about.open')}
          </Button>
      </div>

      <div class="setting-row">
          <div>
            <div class="setting-title">{t('settings.about.license')}</div>
            <div class="setting-hint">{t('settings.about.license.hint')}</div>
          </div>
      </div>

      <div class="diag-block last-errors">
        <div class="diag-head">
          {t('settings.about.recentErrors')}<span class="dim">{t('settings.about.recentErrors.note')}</span>
        </div>
        {#if recentErrors.length === 0}
          <div class="dim">{t('settings.about.noErrors')}</div>
        {:else}
          <ul class="error-list">
            {#each recentErrors.slice().reverse() as item}
              <li>
                <span class="dim">{formatDateTime(item.atMs)}</span>
                <span class="danger">{item.code}</span>
                {item.message}
                {#if item.detail}
                  <span class="dim" title={item.detail}>{t('settings.about.hasDetail')}</span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- 支持开发：独立区块置于「关于」末尾并居中，不配描述文案。结构按多二维码并排预留：新增平台时在 .support-items 内并列一个 .support-item
      即可。爱发电二维码使用静态图而非外链，因为打开外部浏览器的能力已由 app_open_url 提供（白名单出站，含爱发电主页）。 -->
      <div class="support-block">
        <div class="support-items">
          <figure class="support-item">
            <button
              class="qr-link"
              type="button"
              title="afdian.com/a/NyaaACG"
              onclick={() => ui.openUrl(AFDIAN_URL)}
            >
              <img
                class="afdian-qr"
                src={afdianQr}
                alt={t('settings.about.afdianQrAlt')}
                width="160"
                height="160"
              />
            </button>
            <figcaption>{t('settings.about.support')}</figcaption>
          </figure>
        </div>
      </div>
    </section>
  </div><!-- /settings-scroll -->
</div>

<style>
  /* 提示文案属于用户可能复制的文本，在全局 user-select:none 下豁免。
     「关于」组的数据目录路径与错误详情同理，一并豁免（diag 样式本体在 overlays.css） */
  .setting-hint,
  .shortcut-foot .dim,
  .datadir-path,
  .diag-block,
  .error-list {
    user-select: text;
    cursor: text;
  }

  /* 版本行的双按钮（releases 图标键 + 获取更新）：setting-row 的 space-between
     会把两个按钮拆到两端，包一层收拢到右侧 */
  .about-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* 平铺区块的组标题：比 setting-title 大一档，是 tab 跳转的落点 */
  .group-title {
    margin: 22px 0 2px;
    font-size: var(--fs-lg);
    font-weight: 600;
    color: var(--text-1);
    scroll-margin-top: 12px;
  }

  .settings-section:first-child .group-title {
    margin-top: 4px;
  }

  /* 统计卡：左侧为图标座（accent 弱底色圆角方块），右侧竖排数值与标签。使用 grid 而非 flex-wrap：设置列宽 720，按内容宽度排列会折成 4 + 1，「索引大小」单独占一行。等宽网格得到 3 + 2，各卡同宽且左缘对齐。minmax(180px)
     使窄窗口自动降为两列，无需额外断点。六项填满 3 × 2。左右 -17px 为出血：卡片自带 1px 边框与 16px 内边距，与内容列叠加后卡内文字比下方设置行多缩进 17px；负 margin 将卡片外缘推出，卡内文字与设置行左缘对齐 */
  .lib-stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
    margin: 12px -17px 14px;
  }

  /* 目录卡同款出血（`LibraryTools` 根元素 `.library-tools` 定义在全局样式表，
     此处需包一层才能应用 scoped 规则） */
  .lib-bleed {
    margin: 0 -17px;
  }

  /* 数据目录路径：等宽 + 超长省略（悬停 title 给全路径） */
  .datadir-cell {
    min-width: 0;
  }

  .datadir-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
  }

  /* 爱发电二维码：自带黑色底，圆角 + 细边框与卡片语言一致。
   .qr-link 是可点外壳：重置按钮默认外观，hover 轻浮提示可点 */
  .qr-link {
    display: block;
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
    transition: opacity var(--dur-fast) var(--ease);
  }

  .qr-link:hover {
    opacity: 0.85;
  }

  .afdian-qr {
    display: block;
    width: 160px;
    height: 160px;
    border-radius: 12px;
    border: 1px solid var(--line);
  }

  /* 支持开发区块：「关于」组末尾居中；.support-items 为并排容器，
     新增平台二维码时并列一个 .support-item（图像 + 一行说明） */
  .support-block {
    margin-top: 18px;
    display: flex;
    justify-content: center;
  }

  .support-items {
    display: flex;
    justify-content: center;
    align-items: flex-start;
    gap: 32px;
  }

  .support-item {
    margin: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }

  .support-item figcaption {
    font-size: var(--fs-base);
    color: var(--text-2);
  }

  /* 最近错误块：diag-block 全局只有 margin-bottom，与上方数据目录行的
     间距由本块的 margin-top 提供 */
  .last-errors {
    margin-top: 14px;
  }

  /* 大标题 + 右侧 tab（与曲库页同构）：头部固定，内容在 .settings-scroll 内滚动。
     tab 只做 scrollIntoView，不做滚动定位，因此不存在 scroll-spy 的抖动 */
  .settings-page {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  /* 居中列（1064 = 内容 1020 + 左右 padding
     22×2）：头部与滚动容器同一列宽，标题、tab、设置行左缘全部对齐；窄窗口 max-width 不生效，行为与全宽时一致 */
  .settings-headwrap {
    max-width: 1064px;
    width: 100%;
    margin: 0 auto;
  }

  .settings-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 22px 28px;
    max-width: 1064px;
    width: 100%;
    margin: 0 auto;
  }

  /* tab 形态与曲库页一致：纯文字 + accent 下划线，小标题档（由 TextTabs 统一） */

  /* 被「全局 3D 效果」闸门关闭的行：整行压暗，与"该项自身关闭"区分。仅给控件加 disabled 不足以表达——关闭态的开关本身即为灰色，两种情况会混淆。使用内联 style:opacity 而非 `.setting-row.muted`
     规则：Svelte 5 的未使用选择器裁剪会把"仅由 class: 指令产生"的类名判为未使用并删除整条规则 （表现为 class 已加上、规则不存在、computed opacity 仍为 1） */

  /* 环境警告（如"仅检测到软件渲染"）：随 hint 排布，需具备足够对比度 */
  .hint-warn {
    display: block;
    margin-top: 3px;
    font-weight: 600;
    color: var(--warn);
  }
</style>
