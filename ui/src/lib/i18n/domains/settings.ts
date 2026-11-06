/** settings 域：设置页（SettingsPanel）的分组、设置项标题/提示、选项文案、
 *  诊断块与关于页。zh 值逐字保留迁移前的中文文案；en 按 docs/64a 术语表与
 *  风格规则新写（sentence case、ASCII 标点、占位符个数顺序不变）。 */

export const settingsZh = {
  // 头部
  'settings.title': '设置',
  'settings.subtitle': '应用偏好与曲库管理',
  'settings.nav.aria': '设置分组',

  // 分组（tab 与 group-title 共用）
  'settings.section.library': '曲库',
  'settings.section.appearance': '外观',
  'settings.section.playback': '播放',
  'settings.section.system': '系统',
  'settings.section.shortcuts': '快捷键',
  'settings.section.about': '关于',

  // 曲库统计卡
  'settings.library.stats.tracks': '曲目',
  'settings.library.stats.albums': '专辑',
  'settings.library.stats.artists': '艺人',
  'settings.library.stats.totalDuration': '总时长',
  'settings.library.stats.fileSize': '文件容量',
  'settings.library.stats.indexSize': '索引大小',
  'settings.library.stats.missingLocations': '位置缺失',

  // 外观
  'settings.appearance.theme': '主题',
  'settings.appearance.theme.hint.auto': '随操作系统的深浅色模式自动切换，系统切换时即时生效',
  'settings.appearance.theme.hint.light': '浅色主题适合白天与投影，切换即时生效',
  'settings.language.title': '界面语言',
  'settings.language.hint': '首次启动按系统语言自动选择，此后跟随此设置',
  'settings.appearance.themeColor': '主题颜色',
  'settings.appearance.themeColor.hint': '渐变背景与强调色成套切换，决定整个应用的视觉基调',
  'settings.appearance.uiScale': '界面缩放',
  'settings.appearance.uiScale.hint':
    '整体放大文字与间距，适配高分屏可读性，100% 为标准大小，切换即时生效',
  'settings.appearance.glEffects': '全局 3D 效果',
  'settings.appearance.glEffects.hint':
    '关闭后三维频谱与动态背景全部停用，只留静态渐变，频谱相关设置一并失效',
  'settings.appearance.glEffects.warn': '本机只检测到软件渲染，建议保持关闭',
  'settings.appearance.bgSpectrum': '背景动态频谱',
  'settings.appearance.bgSpectrum.hint.on':
    '整个应用的背景随音乐起伏，呈现山形环境光；关闭后只剩静态渐变',
  'settings.appearance.offByGl': '已随「全局 3D 效果」关闭',
  'settings.appearance.bgSpectrumStyle': '背景频谱形态',
  'settings.appearance.bgSpectrumStyle.hint.on':
    '山形：底部一团起伏的环境光；波纹：一层层等深线铺满下半屏，与「正在播放」页的形态各自独立',
  'settings.appearance.ridgeGlow': '背景频谱光效',
  'settings.appearance.ridgeGlow.hint.on':
    '控制光效的强度与锐度：柔雾只有形状没有光，平衡在边缘带一点光，锐光明亮清晰',
  'settings.appearance.npSpectrum': '正在播放页频谱',
  'settings.appearance.npSpectrum.hint.on':
    '只在「正在播放」页显示的可视化，与背景环境光各自独立；关掉不影响背景',
  'settings.appearance.spectrumStyle': '频谱形态',
  'settings.appearance.spectrumStyle.hint':
    '「正在播放」页的频谱形状：镜像沿中线对称，环带绕圆一周，残影留余波轨迹，波纹山谷交替，切换即时生效',

  // 输出模式（含分段控件选项；'null' 静音档不进分段控件，仅供 hint 查询）
  'settings.output.title': '输出模式',
  'settings.output.hint.fallback': '共享：与系统混音，其他程序照常出声',
  'settings.output.mode.shared': '共享',
  'settings.output.mode.shared.hint': '与系统混音，其他程序照常出声',
  'settings.output.mode.exclusive': '独占',
  'settings.output.mode.exclusive.hint': '优先独占，设备拒绝时自动改用共享',
  'settings.output.mode.exclusiveRequired': '独占(强)',
  'settings.output.mode.exclusiveRequired.hint': '必须独占，设备拒绝时直接报错',
  // Web 宿主静态说明行（backend-caps 隐掉输出整组后交代声音从哪来）
  'settings.output.webDecode.title': '音频输出',
  'settings.output.webDecode.hint': '由浏览器解码播放（HTTP Range + Web Audio），服务端不解码音频',

  // 播放
  'settings.playback.buffer': '缓冲',
  'settings.playback.buffer.hint': '缓冲越深越抗卡顿、延迟越大，自动档按输出模式取默认值',
  'settings.playback.buffer.auto': '自动',
  'settings.playback.clock': '时钟',
  'settings.playback.clock.hint':
    '自动：独占输出先试事件、失败再试轮询；共享输出固定用事件，更省电也更稳',
  'settings.playback.clock.auto': '自动',
  'settings.playback.clock.events': '事件',
  'settings.playback.clock.polling': '轮询',
  'settings.playback.releaseOnPause': '暂停时释放设备',
  'settings.playback.releaseOnPause.hint':
    '暂停时把独占的音频设备让回给系统，其他程序可以出声；恢复播放会重新占用，可能有短暂停顿',
  'settings.playback.autoplayOnStart': '启动时自动播放',
  'settings.playback.autoplayOnStart.hint': '打开应用就接着放上次那一首；关闭则停在暂停状态',
  'settings.playback.rememberPosition': '记住播放进度',
  'settings.playback.rememberPosition.hint': '下次启动从上次停下的位置继续；关闭则每次从头开始',
  'settings.playback.preventSleep': '播放时阻止系统休眠',
  'settings.playback.preventSleep.hint': '只阻止系统睡眠，屏幕仍会按系统设置熄掉',
  'settings.playback.normalizeVolume': '音量归一化',
  'settings.playback.normalizeVolume.hint':
    '拉平不同曲目之间的响度差异，不改变你设置的音量，只在播放时做补偿',

  // 系统
  'settings.system.disableSystemScale': '禁用系统缩放',
  'settings.system.disableSystemScale.hint':
    '忽略 Windows 的显示缩放，按 100% 渲染：文字变小、一屏内容更多；与「外观 → 界面缩放」叠加生效，改后立即应用无需重启',
  'settings.system.closeToTray': '关闭窗口时最小化到托盘',
  'settings.system.closeToTray.hint': '打开后点关闭只是隐藏窗口，播放继续；要退出请用托盘菜单',
  'settings.system.autoRefresh': '曲库自动刷新',
  'settings.system.autoRefresh.hint':
    '在别的软件里改了标签或移动了文件，播放器隔多久自己发现；关闭后仍会在切回窗口时刷一次',
  'settings.system.autoRefresh.control': '曲库自动刷新间隔',
  'settings.system.refresh.off': '关闭',
  'settings.system.refresh.30s': '30 秒',
  'settings.system.refresh.2m': '2 分钟',
  'settings.system.refresh.5m': '5 分钟',
  'settings.system.historyRetention': '最近播放保留',
  'settings.system.historyRetention.hint':
    '播放记录只保留这段时间，到期自动清理；「最近播放」列表与常听都按这个范围统计',
  'settings.system.historyRetention.control': '最近播放保留周期',
  'settings.system.history.30d': '30 天',
  'settings.system.history.90d': '90 天',
  'settings.system.history.halfYear': '半年',
  'settings.system.history.year': '一年',
  'settings.system.history.forever': '永久',

  // 快捷键
  'settings.shortcuts.footHint': 'Esc 取消录制，Backspace 清空绑定；↑/↓ 与 Enter 是列表导航，不占用快捷键',
  'settings.shortcuts.reset': '恢复默认',
  'settings.shortcuts.conflict': '「{binding}」同时绑定多个动作：{actions}',
  'settings.shortcuts.actionSep': '、',
  'settings.shortcuts.pressNewKey': '按下新键…',

  // 关于
  'settings.about.version': '版本',
  'settings.about.checkUpdates': '获取更新',
  'settings.about.checkUpdates.title': '获取更新功能尚未提供',
  'settings.about.dataDir': '数据目录',
  'settings.about.openDir': '打开目录',
  'settings.about.repo': '源码仓库',
  'settings.about.license': '许可证',
  'settings.about.license.hint': 'GPL-3.0-or-later',
  'settings.about.open': '打开',
  'settings.about.releases.open': '打开发布页',
  'settings.about.recentErrors': '最近错误',
  'settings.about.recentErrors.note': '最多保留 30 条，新的在上',
  'settings.about.noErrors': '没有错误',
  'settings.about.hasDetail': '有详情',
  'settings.about.afdianQrAlt': '爱发电赞助二维码',
  'settings.about.support': '支持创作，用爱发电',

  // 分段控件选项标签：lib/settings.ts 的 THEME_MODES / AMBIENT_SCHEMES /
  // SPECTRUM_STYLES / RIDGE_GLOWS / BG_SPECTRUM_STYLES 的 label 存本域键，
  // SettingsPanel 渲染时 t()（UI_SCALE_OPTIONS 的 `${scale}%` 语言中立，不进表）
  'settings.themeMode.dark': '深色',
  'settings.themeMode.light': '浅色',
  'settings.themeMode.system': '跟随系统',
  'settings.ambient.aurora': '极光',
  'settings.ambient.ember': '流霞',
  'settings.ambient.meadow': '苔原',
  'settings.ambient.dusk': '暮紫',
  'settings.ambient.mono': '素白',
  'settings.spectrum.mirror': '镜像',
  'settings.spectrum.radial': '环带',
  'settings.spectrum.ripple': '残影',
  'settings.spectrum.wave': '波纹',
  'settings.ridgeGlow.soft': '柔雾',
  'settings.ridgeGlow.balanced': '平衡',
  'settings.ridgeGlow.neon': '锐光',
  'settings.bgSpectrumStyle.ridge': '山形',
  'settings.bgSpectrumStyle.ripple': '波纹',
} as const;

export const settingsEn: Record<keyof typeof settingsZh, string> = {
  'settings.title': 'Settings',
  'settings.subtitle': 'App preferences & library management',
  'settings.nav.aria': 'Settings sections',

  'settings.section.library': 'Library',
  'settings.section.appearance': 'Appearance',
  'settings.section.playback': 'Playback',
  'settings.section.system': 'System',
  'settings.section.shortcuts': 'Shortcuts',
  'settings.section.about': 'About',

  'settings.library.stats.tracks': 'Tracks',
  'settings.library.stats.albums': 'Albums',
  'settings.library.stats.artists': 'Artists',
  'settings.library.stats.totalDuration': 'Total duration',
  'settings.library.stats.fileSize': 'File size',
  'settings.library.stats.indexSize': 'Index size',
  'settings.library.stats.missingLocations': 'Missing locations',

  'settings.appearance.theme': 'Theme',
  'settings.appearance.theme.hint.auto':
    'Follows the operating system light or dark mode and applies instantly when the system switches',
  'settings.appearance.theme.hint.light':
    'The light theme suits daylight and projection; changes apply instantly',
  'settings.language.title': 'Interface language',
  'settings.language.hint': 'Chosen from your system language on first launch; changes apply immediately',
  'settings.appearance.themeColor': 'Theme color',
  'settings.appearance.themeColor.hint':
    'Gradient background and accent color switch as a set, defining the look of the whole app',
  'settings.appearance.uiScale': 'UI scale',
  'settings.appearance.uiScale.hint':
    'Scales text and spacing for high-resolution screens; 100% is the standard size and changes apply instantly',
  'settings.appearance.glEffects': 'Global 3D effects',
  'settings.appearance.glEffects.hint':
    'When off, the 3D spectrum and dynamic background are disabled entirely, leaving static gradients, and spectrum settings stop applying',
  'settings.appearance.glEffects.warn': 'Only software rendering detected on this machine; keep this off',
  'settings.appearance.bgSpectrum': 'Dynamic background spectrum',
  'settings.appearance.bgSpectrum.hint.on':
    'The app background rises and falls with the music as a mountain-shaped ambient glow; when off only static gradients remain',
  'settings.appearance.offByGl': 'Disabled with "Global 3D effects"',
  'settings.appearance.bgSpectrumStyle': 'Background spectrum style',
  'settings.appearance.bgSpectrumStyle.hint.on':
    'Mountain: one rolling glow along the bottom; ripple: layers of contour lines filling the lower half of the screen, independent of the Now Playing style',
  'settings.appearance.ridgeGlow': 'Background spectrum glow',
  'settings.appearance.ridgeGlow.hint.on':
    'Controls glow intensity and sharpness: Soft is shape without light, Balanced adds a slight edge glow, Sharp is bright and crisp',
  'settings.appearance.npSpectrum': 'Now Playing spectrum',
  'settings.appearance.npSpectrum.hint.on':
    'A visualization shown only on the Now Playing page, independent of the background glow; turning it off does not affect the background',
  'settings.appearance.spectrumStyle': 'Spectrum style',
  'settings.appearance.spectrumStyle.hint':
    'The spectrum shape on the Now Playing page: mirror is symmetric about the center line, ring wraps a full circle, trail leaves fading paths, ripple alternates peaks and valleys; changes apply instantly',

  'settings.output.title': 'Output mode',
  'settings.output.hint.fallback': 'Shared: mixes with the system; other apps keep playing sound',
  'settings.output.mode.shared': 'Shared',
  'settings.output.mode.shared.hint': 'Mixes with the system; other apps keep playing sound',
  'settings.output.mode.exclusive': 'Exclusive',
  'settings.output.mode.exclusive.hint':
    'Prefers exclusive mode and falls back to shared mode when the device refuses',
  'settings.output.mode.exclusiveRequired': 'Exclusive (forced)',
  'settings.output.mode.exclusiveRequired.hint':
    'Requires exclusive mode; errors out when the device refuses',
  'settings.output.webDecode.title': 'Audio output',
  'settings.output.webDecode.hint':
    'Decoded and played by the browser (HTTP Range + Web Audio); the server never touches audio',

  'settings.playback.buffer': 'Buffer',
  'settings.playback.buffer.hint':
    'A deeper buffer resists stutter but adds latency; Auto picks the default for the output mode',
  'settings.playback.buffer.auto': 'Auto',
  'settings.playback.clock': 'Clock',
  'settings.playback.clock.hint':
    'Auto: exclusive output tries events first, then polling on failure; shared output always uses events, which saves power and is more stable',
  'settings.playback.clock.auto': 'Auto',
  'settings.playback.clock.events': 'Events',
  'settings.playback.clock.polling': 'Polling',
  'settings.playback.releaseOnPause': 'Release device when paused',
  'settings.playback.releaseOnPause.hint':
    'While paused, hands the exclusive audio device back to the system so other apps can play; resuming takes the device again and may pause briefly',
  'settings.playback.autoplayOnStart': 'Autoplay on startup',
  'settings.playback.autoplayOnStart.hint': 'Resumes the last track when the app opens; when off it stays paused',
  'settings.playback.rememberPosition': 'Remember playback position',
  'settings.playback.rememberPosition.hint':
    'Next launch continues from where you left off; when off playback starts from the beginning',
  'settings.playback.preventSleep': 'Prevent sleep while playing',
  'settings.playback.preventSleep.hint': 'Only prevents system sleep; the screen still turns off per system settings',
  'settings.playback.normalizeVolume': 'Volume normalization',
  'settings.playback.normalizeVolume.hint':
    'Evens out loudness differences between tracks without changing your volume setting; the compensation only happens during playback',

  'settings.system.disableSystemScale': 'Disable system scaling',
  'settings.system.disableSystemScale.hint':
    'Ignores Windows display scaling and renders at 100%: smaller text, more content per screen; stacks with "Appearance > UI scale" and applies immediately without a restart',
  'settings.system.closeToTray': 'Minimize to tray on close',
  'settings.system.closeToTray.hint':
    'With this on, closing the window only hides it and playback continues; use the tray menu to quit',
  'settings.system.autoRefresh': 'Library auto-refresh',
  'settings.system.autoRefresh.hint':
    'How often the player notices tags edited or files moved in other software; when off it still refreshes once when you switch back to the window',
  'settings.system.autoRefresh.control': 'Library auto-refresh interval',
  'settings.system.refresh.off': 'Off',
  'settings.system.refresh.30s': '30 s',
  'settings.system.refresh.2m': '2 min',
  'settings.system.refresh.5m': '5 min',
  'settings.system.historyRetention': 'Recently played retention',
  'settings.system.historyRetention.hint':
    'Play history is kept only for this period and cleaned up automatically; "Recently played" and "Most played" are both counted within this range',
  'settings.system.historyRetention.control': 'Recently played retention period',
  'settings.system.history.30d': '30 days',
  'settings.system.history.90d': '90 days',
  'settings.system.history.halfYear': '6 months',
  'settings.system.history.year': '1 year',
  'settings.system.history.forever': 'Forever',

  'settings.shortcuts.footHint':
    'Esc cancels recording, Backspace clears the binding; Up/Down and Enter are list navigation and are not captured as shortcuts',
  'settings.shortcuts.reset': 'Reset to defaults',
  'settings.shortcuts.conflict': '"{binding}" is bound to multiple actions: {actions}',
  'settings.shortcuts.actionSep': ', ',
  'settings.shortcuts.pressNewKey': 'Press a new key…',

  'settings.about.version': 'Version',
  'settings.about.checkUpdates': 'Check for updates',
  'settings.about.checkUpdates.title': 'Update checking is not available yet',
  'settings.about.dataDir': 'Data folder',
  'settings.about.openDir': 'Open folder',
  'settings.about.repo': 'Repository',
  'settings.about.license': 'License',
  'settings.about.license.hint': 'GPL-3.0-or-later',
  'settings.about.open': 'Open',
  'settings.about.releases.open': 'Open releases',
  'settings.about.recentErrors': 'Recent errors',
  'settings.about.recentErrors.note': 'Keeps up to 30 entries, newest first',
  'settings.about.noErrors': 'No errors',
  'settings.about.hasDetail': 'Details available',
  'settings.about.afdianQrAlt': 'Afdian donation QR code',
  'settings.about.support': 'Support development on Afdian',

  'settings.themeMode.dark': 'Dark',
  'settings.themeMode.light': 'Light',
  'settings.themeMode.system': 'Follow system',
  'settings.ambient.aurora': 'Aurora',
  'settings.ambient.ember': 'Ember',
  'settings.ambient.meadow': 'Meadow',
  'settings.ambient.dusk': 'Dusk',
  'settings.ambient.mono': 'Mono',
  'settings.spectrum.mirror': 'Mirror',
  'settings.spectrum.radial': 'Ring',
  'settings.spectrum.ripple': 'Trail',
  'settings.spectrum.wave': 'Ripple',
  'settings.ridgeGlow.soft': 'Soft',
  'settings.ridgeGlow.balanced': 'Balanced',
  'settings.ridgeGlow.neon': 'Sharp',
  'settings.bgSpectrumStyle.ridge': 'Mountain',
  'settings.bgSpectrumStyle.ripple': 'Ripple',
};
