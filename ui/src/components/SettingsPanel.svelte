<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Clock3, Database, Disc3, ListMusic, Mic2, TriangleAlert } from '@lucide/svelte';
  import type {
    AppSettings,
    AmbientScheme,
    BgSpectrumStyle,
    RidgeGlow,
    SpectrumStyle,
    ThemeMode,
  } from '../lib/settings';
  import { AMBIENT_SCHEMES, BG_SPECTRUM_STYLES, RIDGE_GLOWS, SPECTRUM_STYLES } from '../lib/settings';
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
   * 识别信息与排障数据（版本/平台/Schema/数据目录/最近错误），
   * 2026-10-28 从扫描入库页的「高级与诊断」折叠区迁来——它们描述的是
   * 整个应用，放在「关于」tab 比挂在入库页更对位。数据同源：
   * `ui.diagnostics`，与系统 tab 的集成状态共用一份。
   */
  const sys = $derived(ui.diagnostics);
  const recentErrors = $derived(sys?.recentErrors ?? []);

  /**
   * 曲库统计（曲目 / 专辑 / 艺人 / 总时长 / 索引大小 / 位置缺失）。
   *
   * 只读：真正的拉取在 `boot.ts` 与每次扫描收尾（`library.loadStats`），
   * 这个区块不自己发请求——设置页打开一次就查一遍库，属于"看页码就查数据库"。
   */
  const stats = $derived(library.stats);

  /**
   * 单页平铺 + tab 跳转（2026-10-28 用户拍板改回）。
   *
   * 演进史：旧版「全区块平铺 + scroll-spy」的病根是点击分组靠
   * scrollIntoView 平滑滚动、滚动中 spy 又不断写回高亮——两边互相拉扯，
   * 切一次组高亮抖好几下。上一版改成 tab 条件渲染，抖动消失了，但用户
   * 不喜欢"设置被切成五页"。现在是折中：**六个区块全部平铺渲染，顶部
   * tab 只做跳转定位**——点击 scrollIntoView，没有 spy；高亮只跟点击走
   * （tab 的语义是"跳过去"，不是"你在这"），滚动永不写回高亮，拉扯
   * 从根上不存在。
   *
   * 2026-10-30：`library` 是新加的一组（原「扫描入库」页整体并入，见下方
   * 该 section 的注释）。排序按"内容 → 应用"：外观/播放/曲库是你在听什么
   * 怎么看，系统/快捷键/关于是这台机器与应用本身。
   */
  type Section = SettingsSection;
  let active = $state<Section>('appearance');

  const SECTIONS: Array<{ id: Section; label: string }> = [
    { id: 'appearance', label: '外观' },
    { id: 'playback', label: '播放' },
    { id: 'library', label: '曲库' },
    { id: 'system', label: '系统' },
    { id: 'shortcuts', label: '快捷键' },
    { id: 'about', label: '关于' },
  ];

  /** 各区块根元素（bind:this 填充），tab 跳转的目标 */
  const sectionEls: Record<Section, HTMLElement | null> = {
    appearance: null,
    playback: null,
    library: null,
    system: null,
    shortcuts: null,
    about: null,
  };

  function jumpTo(section: Section): void {
    active = section;
    sectionEls[section]?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  /**
   * 消费 `nav.settingsAt()` 挂上的一次性跳转请求（曲库页「位置缺失」告警用）。
   *
   * 为什么 `await tick()`：请求是在**路由切换之前**写进去的，本组件可能是
   * 这一帧才第一次挂载——那时 `sectionEls` 还是空对象，直接 `scrollIntoView`
   * 会静默什么都不做（`.?.` 吃掉）。tick 之后 bind:this 已填好。
   *
   * 读后立刻清空：这是一次性请求，不是"当前分组"状态（后者会让滚动与 tab
   * 高亮重新互相拉扯，正是这页上一版回退掉的设计）。
   */
  $effect(() => {
    const want = ui.settingsSection;
    if (!want) return;
    ui.settingsSection = null;
    void tick().then(() => jumpTo(want));
  });

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

  /**
   * 全局 3D 闸门（docs/46 §6.2）：关掉后虽然 `bgSpectrum` / `npSpectrum` 的**值**
   * 还在设置里，但一律不生效。UI 上必须把下面几项置灰而不是隐藏——用户要能看见
   * "我关掉的是哪些东西"，隐藏会让人以为功能消失了。
   */
  const gl = $derived(settings.glEffects);

  /**
   * 本机 3D 能力（探测结果有缓存，打开设置页只探一次）。
   *
   * 放在这里的理由：`glEffects` 是个用户看不懂后果的开关——"我的机器到底行不行"
   * 应该由应用直接说出来，而不是让用户自己试。软件渲染（远程桌面 / 关了硬件加速）
   * 下开着它只会更卡，所以 hint 要明确劝关。
   */
  const cap = detectCapability();

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

  /**
   * 自动刷新间隔（秒）。0 = 只保留"切回窗口时刷新"。
   *
   * 上限只给到 5 分钟：再慢就不如关掉定时（切回窗口已经覆盖了主要场景），
   * 留一个"每半小时想起来才扫一次"的档位只会让用户以为它坏了。
   */
  const REFRESH_OPTIONS: Array<{ value: string; label: string }> = [
    { value: '0', label: '关闭' },
    { value: '30', label: '30 秒' },
    { value: '120', label: '2 分钟' },
    { value: '300', label: '5 分钟' },
  ];

  /** 播放记录保留周期（0 = 永久）；宿主据此裁剪 history 行 */
  const HISTORY_DAYS_OPTIONS: Array<{ value: string; label: string }> = [
    { value: '30', label: '30 天' },
    { value: '90', label: '90 天' },
    { value: '180', label: '半年' },
    { value: '365', label: '一年' },
    { value: '0', label: '永久' },
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
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="settings-page">
  <!-- 头部与内容同在一根居中列（2026-10-28 用户拍板）：设置是表单式页面，
       内容列 1020 上限后左侧留白很大，标题还贴屏幕左会显得割裂 -->
  <div class="settings-headwrap">
    <div class="page-pad">
      <!-- 大标题固定「设置」+ 右侧 tab 只做跳转（2026-10-28 用户拍板改回单页） -->
      <Header title="设置" big>
        {#snippet actions()}
          <!-- 语义用 nav + aria-current 而不是 role="tablist"/"tab"（同 content/Toolbar
               的先例）：tab 只做 scrollIntoView 跳转，五个区块平铺常驻，不是「切换
               面板」；硬套 tab 角色会让读屏软件期待一个 tabpanel。 -->
          <nav class="settings-tabs" aria-label="设置分组">
            {#each SECTIONS as section (section.id)}
              <button
                class="settings-tab"
                class:on={active === section.id}
                aria-current={active === section.id ? 'true' : undefined}
                onclick={() => jumpTo(section.id)}
              >
                {section.label}
              </button>
            {/each}
          </nav>
        {/snippet}
      </Header>
    </div>
  </div>

  <!-- 内容容器自己滚：头部（标题+tab）固定。五个区块全部平铺渲染，
       tab 点击只 scrollIntoView 跳转（2026-10-28 用户拍板改回单页） -->
  <div class="settings-scroll">
    <section class="settings-section" bind:this={sectionEls.appearance}>
      <h3 class="group-title">外观</h3>
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
            <div class="setting-title">主题颜色</div>
            <div class="setting-hint">渐变背景与强调色成套切换，决定整个应用的视觉基调</div>
          </div>
          <SegmentedControl
            label="主题颜色"
            value={settings.ambient}
            options={AMBIENT_SCHEMES}
            onchange={(value) => apply({ ambient: value as AmbientScheme })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">全局 3D 效果</div>
            <div class="setting-hint">
              总开关：关闭后所有三维频谱与流动背景停用，只留静态渐变；下面的频谱项随之失效
              {#if !cap.hardware}
                <strong class="hint-warn">本机只检测到软件渲染，建议保持关闭</strong>
              {/if}
            </div>
          </div>
          <Switch
            label="全局 3D 效果"
            checked={settings.glEffects}
            onchange={(checked) => apply({ glEffects: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">背景动态频谱</div>
            <div class="setting-hint">
              {gl
                ? '整个应用的背景有随音乐起伏的山形环境光；关闭后只剩静态渐变，引擎 tap 停止、零开销'
                : '已随「全局 3D 效果」关闭'}
            </div>
          </div>
          <Switch
            label="背景动态频谱"
            checked={settings.bgSpectrum}
            disabled={!gl}
            onchange={(checked) => apply({ bgSpectrum: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">背景频谱形态</div>
            <div class="setting-hint">
              {gl
                ? '山形：底部一团起伏的环境光；波纹：一层层等深线铺满下半屏。两者用的都是同一份频谱，与「正在播放」页的形态各自独立'
                : '已随「全局 3D 效果」关闭'}
            </div>
          </div>
          <SegmentedControl
            label="背景频谱形态"
            value={settings.bgSpectrumStyle}
            options={BG_SPECTRUM_STYLES}
            disabled={!gl}
            onchange={(value) => apply({ bgSpectrumStyle: value as BgSpectrumStyle })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">背景频谱光效</div>
            <div class="setting-hint">
              {gl
                ? '山形与波纹共用这一档：柔雾只有形状没有光，平衡在边缘带一点光，锐光把脊线/线心描出来'
                : '已随「全局 3D 效果」关闭'}
            </div>
          </div>
          <SegmentedControl
            label="背景频谱光效"
            value={settings.ridgeGlow}
            options={RIDGE_GLOWS}
            disabled={!gl}
            onchange={(value) => apply({ ridgeGlow: value as RidgeGlow })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">正在播放页频谱</div>
            <div class="setting-hint">
              {gl
                ? '只在「正在播放」页显示的可视化，与背景环境光各自独立；关掉不影响背景'
                : '已随「全局 3D 效果」关闭'}
            </div>
          </div>
          <Switch
            label="正在播放页频谱"
            checked={settings.npSpectrum}
            disabled={!gl}
            onchange={(checked) => apply({ npSpectrum: checked })}
          />
        </div>

        <div class="setting-row" style:opacity={gl ? 1 : 0.5}>
          <div>
            <div class="setting-title">频谱形态</div>
            <div class="setting-hint">
              「正在播放」页可视化的形态：镜像曲线（中线上下对称）、径向环形（绕圆一周）、中轴波纹（一条居中曲线），切换即时生效
            </div>
          </div>
          <SegmentedControl
            label="频谱形态"
            value={settings.spectrumStyle}
            options={SPECTRUM_STYLES}
            disabled={!gl}
            onchange={(value) => apply({ spectrumStyle: value as SpectrumStyle })}
          />
        </div>
      </section>

    <section class="settings-section" bind:this={sectionEls.playback}>
      <h3 class="group-title">播放</h3>
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

    <!--
      曲库（2026-10-30）：原「扫描入库」独立页整体并入。

      为什么拆掉那一页：beets 拆成独立「整理库」页之后（docs/48），那一页
      只剩"本应用的曲库怎么进来"一件事——一个侧栏一级席位换一条卡片，代价
      太大。曲库目录本身是**配置**（跟"最近播放保留多久"同性质），归设置页。

      与原来那一页的差别只有排版：不再有页面大标题与「曲库」小结，改用设置页
      统一的 group-title；统计卡与卡片原样搬过来（`components/LibraryTools`
      本组件一行引用即可，它自己不依赖页面壳）。
    -->
    <section class="settings-section" id="settings-library" bind:this={sectionEls.library}>
      <h3 class="group-title">曲库</h3>

      <!-- 统计卡：图标 + 大数字 + 标签。数字 tabular 不抖；图标用中性描边件，
           不抢数字的戏。索引大小不是"音乐文件多大"，是 SQLite 文件自己的体积 -->
      {#if stats}
        <div class="lib-stats">
          <div class="stat-card">
            <span class="stat-icon"><ListMusic size={17} /></span>
            <div class="stat-text"><b>{stats.tracks}</b><span>曲目</span></div>
          </div>
          <div class="stat-card">
            <span class="stat-icon"><Disc3 size={17} /></span>
            <div class="stat-text"><b>{stats.albums}</b><span>专辑</span></div>
          </div>
          <div class="stat-card">
            <span class="stat-icon"><Mic2 size={17} /></span>
            <div class="stat-text"><b>{stats.artists}</b><span>艺人</span></div>
          </div>
          <div class="stat-card">
            <span class="stat-icon"><Clock3 size={17} /></span>
            <div class="stat-text"><b>{formatDurationLong(stats.totalDurationMs)}</b><span>总时长</span></div>
          </div>
          <div class="stat-card">
            <span class="stat-icon"><Database size={17} /></span>
            <div class="stat-text"><b>{formatBytes(stats.dbBytes)}</b><span>索引大小</span></div>
          </div>
          {#if stats.missingLocations > 0}
            <div class="stat-card warn">
              <span class="stat-icon"><TriangleAlert size={17} /></span>
              <div class="stat-text"><b>{stats.missingLocations}</b><span>位置缺失</span></div>
            </div>
          {/if}
        </div>
      {/if}

      <LibraryTools />
    </section>

    <section class="settings-section" bind:this={sectionEls.system}>
      <h3 class="group-title">系统</h3>
      <div class="setting-row">
          <div>
            <div class="setting-title">关闭窗口时最小化到托盘</div>
            <div class="setting-hint">
              打开后点关闭只是隐藏窗口，播放继续；要退出请用托盘菜单或 Ctrl+Q
              {#if sys}<span class="state-tail">当前：{sys.tray ? '已创建' : '未创建'}</span>{/if}
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
            <div class="setting-title">曲库自动刷新</div>
            <div class="setting-hint">
              在别的软件里改了标签或移动了文件，播放器隔多久自己发现。
              关闭后仍会在切回窗口时刷一次
            </div>
          </div>
          <SegmentedControl
            label="曲库自动刷新间隔"
            value={String(settings.refreshSeconds ?? 30)}
            options={REFRESH_OPTIONS}
            onchange={(value) => apply({ refreshSeconds: Number(value) })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">最近播放保留</div>
            <div class="setting-hint">
              播放记录只存这段时间内的（到点自动清理，防数据无限膨胀）。
              「最近播放」列表与常听榜都按这个范围统计
            </div>
          </div>
          <SegmentedControl
            label="最近播放保留周期"
            value={String(settings.historyDays ?? 90)}
            options={HISTORY_DAYS_OPTIONS}
            onchange={(value) => apply({ historyDays: Number(value) })}
          />
        </div>

        <div class="setting-row">
          <div>
            <div class="setting-title">系统媒体键</div>
            <div class="setting-hint">
              播放器的标题、封面与播放状态同步到系统媒体面板，媒体键由系统路由，失焦也生效
              {#if sys}
                <span class="state-tail">
                  当前：{!sys.mediaKeys.enabled
                    ? '已关闭'
                    : sys.mediaKeys.active.length > 0
                      ? '已生效'
                      : '未生效（还没有播放会话）'}
                </span>
              {/if}
            </div>
          </div>
          <Switch
            label="系统媒体键"
            checked={settings.mediaKeys}
            onchange={(checked) => apply({ mediaKeys: checked })}
          />
        </div>
      </section>

    <section class="settings-section" bind:this={sectionEls.shortcuts}>
      <h3 class="group-title">快捷键</h3>

      <!--
        录制规则 + 恢复默认（2026-10-28 十二轮拍板）：盒子放组顶——用户
        点下任何绑定键之前就该知道 Esc/Backspace 的行为；边框套住让它像
        一条"操作须知"，而不是飘在列表底部的尾注。
      -->
      <div class="shortcut-foot">
        <span class="dim">Esc 取消录制，Backspace 清空绑定。↑/↓ 与 Enter 是列表导航，不占用快捷键。</span>
        <Button size="sm" onclick={() => apply({ shortcuts: {} })}>恢复默认</Button>
      </div>

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
      </section>

    <!--
      关于（识别信息 + 排障，从扫描入库页迁来）。2026-10-28 九轮半拍板：
      改成与设置项同款的「一行一项」；平台/Schema/播放列表计数三行删掉
      （内部信息或恒为 0）；「重新读取」删掉（诊断随页面加载自动拉）；
      版本行带「获取更新」占位按钮（功能未做，恒禁用）；数据目录与
      「打开目录」并成一行。
    -->
    <section class="settings-section settings-about" bind:this={sectionEls.about}>
      <h3 class="group-title">关于</h3>
      <div class="setting-row">
        <div>
          <div class="setting-title">版本</div>
          <div class="setting-hint">{sys?.version ?? '…'}</div>
        </div>
        <Button size="sm" disabled title="获取更新功能尚未提供">获取更新</Button>
      </div>

      <div class="setting-row">
        <div class="datadir-cell">
          <div class="setting-title">数据目录</div>
          <!-- 完整路径可复制（user-select 豁免见样式区），过长省略、悬停给全路径 -->
          <div class="setting-hint datadir-path" title={sys?.dataDir ?? ''}>{sys?.dataDir ?? '…'}</div>
        </div>
        <Button size="sm" onclick={() => ui.openDataDir()}>打开目录</Button>
      </div>

      <div class="diag-block last-errors">
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

      <!--
        支持开发（2026-10-28 十一轮拍板）：独立区块放「关于」最下面，居中，
        不配描述文案——图就是文案。结构按"多二维码并排"预留：以后加别的
        平台，在 .support-items 里并排多一个 .support-item 即可。
        爱发电二维码是静态图，不做外链——Tauri 开浏览器要走 shell open，
        等真做「获取更新」时一起把外链基建搭起来。
      -->
      <div class="support-block">
        <div class="support-items">
          <figure class="support-item">
            <img class="afdian-qr" src={afdianQr} alt="爱发电赞助二维码" width="160" height="160" />
            <figcaption>支持创作，用爱发电</figcaption>
          </figure>
        </div>
      </div>
    </section>
  </div><!-- /settings-scroll -->
</div>

<style>
  /* docs/19 §5：提示文案是用户可能要复制的内容，从全局 user-select:none
     里豁免（diag 样式本体在全局 overlays.css；「关于」的路径与错误详情
     同样是用户可能要复制的内容，一并豁免） */
  .setting-hint,
  .shortcut-foot .dim,
  .datadir-path,
  .diag-block,
  .error-list {
    user-select: text;
    cursor: text;
  }

  /* 单页平铺的区块小标题（2026-10-28 改回）：比 setting-title 大一档，
     与 tab 的关系是"tab 跳到这" */
  .group-title {
    margin: 22px 0 2px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-1);
    scroll-margin-top: 12px;
  }

  .settings-section:first-child .group-title {
    margin-top: 4px;
  }

  /* 统计卡（2026-10-28 七轮拍板：放大字号 + 图标 + 卡片感，随「扫描入库」
     页并入设置页时原样搬来）：左侧图标座（accent 弱底色圆角方块），右侧
     大数字 + 小标签竖排。

     为什么用 grid 而不是原来那条 flex-wrap：设置列只有 720 宽，五张卡按
     内容宽度排会折成"4 + 1"——"索引大小"孤零零占一行，像布局塌了。等宽
     网格下是 3 + 2，每张卡同宽、左缘成列，看着是有意的两行。minmax(180px)
     让窄窗口自动降到两列，不用再写断点。 */
  .lib-stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
    margin: 12px 0 14px;
  }

  .stat-card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 18px 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 12px;
    min-width: 118px;
  }

  .stat-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent-text);
  }

  .stat-card.warn .stat-icon {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }

  .stat-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .stat-text b {
    font-size: 21px;
    font-weight: 400; /* 用户拍板：数字不加粗，靠字号本身立住 */
    line-height: 1.15;
    color: var(--text-1);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .stat-text span {
    font-size: 11.5px;
    color: var(--text-2);
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

  /* 爱发电二维码：自带黑色底，圆角 + 细边框与卡片语言一致 */
  .afdian-qr {
    display: block;
    width: 160px;
    height: 160px;
    border-radius: 12px;
    border: 1px solid var(--line);
  }

  /* 支持开发区块：「关于」末尾居中展示；.support-items 是并排容器，
     以后加别的平台二维码就并排多一个 .support-item（图 + 一行小字） */
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
    font-size: 12px;
    color: var(--text-2);
  }

  /* 最近错误块：diag-block 全局只有 margin-bottom，与上方数据目录行的
     间距落在块自己的 margin-top 上 */
  .last-errors {
    margin-top: 14px;
  }

  /* 大标题 + 右侧 tab（2026-10-28，与曲库页同构）：头部固定不滚，
     内容在 .settings-scroll 容器里滚——tab 是条件渲染，没有滚动定位，
     旧 scroll-spy 的抖动从根上消失 */
  .settings-page {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  /* 居中列（1064 = 内容 1020 + 左右 padding 22×2）：头部与滚动容器同一
     列宽，标题、tab、设置行左缘全部对齐；窄窗口 max-width 不生效，
     行为与全宽时一致 */
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

  /* tab 形态与曲库页 .lib-tab 同款：纯文字 + accent 下划线，小标题档 */
  .settings-tabs {
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-left: 18px;
  }

  .settings-tab {
    padding: 2px 0;
    border: none;
    background: none;
    font-size: 12.5px;
    color: var(--text-2);
    border-bottom: 2px solid transparent;
  }

  .settings-tab:hover {
    color: var(--text-1);
  }

  .settings-tab.on {
    color: var(--text-1);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  /* 被「全局 3D 效果」闸门关掉的行：整行压暗，与"这一项自己关着"区分开。
   * 只给控件加 disabled 不够——关闭态的开关本来也是灰的，两者会混成一片。
   * 用内联 style:opacity 而不是 `.setting-row.muted` 规则：Svelte 5 的未使用
   * 选择器裁剪会把"只由 class: 指令产生"的类名判成没用过，规则整条被删
   * （实测：class 加上了、样式表里查不到这条规则、computed opacity 仍是 1）。 */

  /* "本机只检测到软件渲染"这类环境警告：跟着 hint 走，但要能一眼看见 */
  .hint-warn {
    display: block;
    margin-top: 3px;
    font-weight: 600;
    color: var(--warn);
  }
</style>
