/** 推流频道管理的中英文文案：频道、歌单、节目表与运行控制。 */

export const stationZh = {
  'station.visibilityBadge': '（{visibility}）',
  'station.oneTrack': '{count} 首',
  'station.weekday': '周{day}',
  'station.localTimezone': '服务器本地时区',
  'station.issueEmptyQueue': '开播前请先添加系统歌单或全曲库。',
  'station.issueMissingSource': '歌单已不存在：{name}',
  'station.issueEmptySource': '歌单没有可播放曲目：{name}',
  'station.issueUncoveredTime': '{day} {time} 未安排节目，请添加常规轮播歌单或补全时段。',
  'station.allLibrary':'全曲库', 'station.librarySource':'系统内置',

  'station.priorityInvalid':'优先级须为 0–65535 的整数', 'station.slotNumber':'时段 {number}',

  'station.emptySlots':'拖选一个范围即可新增时段，也可点击新增后直接编辑。',
  'station.nameRequired':'频道名称（必填）', 'station.visibilityRequired':'可见性（必选）',
  'station.formHint':'填写频道信息，标注为选填的内容可留空。', 'station.editInfo':'编辑频道信息',
  'station.workbenchSubtitle':'频道歌单与每周播出安排',
  'station.dragSchedule':'拖选周时间表', 'station.newSlot':'新增时段',

  'station.systemPlaylists':'系统歌单', 'station.broadcastQueue':'频道播放队列',
  'station.smart':'动态', 'station.manual':'手动', 'station.addPlaylist':'添加 {name}', 'station.addedPlaylist':'{name} 已添加',
  'station.queueHint':'常规项按队列顺序逐个播放一轮，指定节目按时段优先播出',
  'station.emptyQueue':'点击上方歌单卡片的加号添加节目', 'station.moveUp':'上移', 'station.moveDown':'下移', 'station.removeQueueItem':'移除 {name}',
  'station.playOrder':'歌单内顺序', 'station.ordered':'按歌单顺序', 'station.shuffle':'随机顺序', 'station.regular':'常规轮播',
  'station.scheduled':'指定时段',

  'station.gridHint':'一格一小时 · 每次拖选新增时段，修改完成后统一保存',
  'station.currentSlot':'当前歌单', 'station.otherSlot':'其他节目',
  'station.timeInvalid':'请选择星期，并填写有效且不相同的起止时间（HH:MM）', 'station.timeConflict':'时段有同优先级重叠，请调整时间或提高优先级',
  'station.loading':'加载中…',
  'station.publicShort':'公开', 'station.privateShort':'私密',
  'station.configure':'配置', 'station.configFor':'{name} 的配置',
  'station.unsaved':'有未保存的修改', 'station.savedPending':'已保存，待应用',
  'station.upToDate':'配置已应用', 'station.saveConfig':'保存配置', 'station.applyHint':'应用时推流可能短暂重连',
  'station.apply':'应用更新', 'station.applied':'配置已应用',
  'station.copyFailed':'复制失败，请检查浏览器剪贴板权限', 'station.deleteStation':'删除频道「{name}」及其节目配置？系统歌单不会删除。',
  'station.unsavedHint':'保存当前修改，或放弃后继续操作。', 'station.keepEditing':'继续编辑', 'station.discard':'放弃修改',
  'station.pageTitle': '推流',
  'station.pageSubtitle': '管理频道、编排歌单与节目表，向收听端持续推流',

  /* 频道列表 */

  'station.new': '新建频道',
  'station.empty': '还没有频道。新建一个，编排歌单与节目表后开播。',

  'station.stateOn': '开播中',
  'station.stateOff': '已停播',

  'station.play': '开播',
  'station.stop': '停播',
  'station.skip': '切歌',

  'station.del': '删除',

  'station.copyHls': '复制收听地址',
  'station.copied': '已复制',

  /* 编辑表单 */
  'station.formTitleNew': '新建频道',

  'station.fieldNamePh': '如「深夜 FM」',
  'station.fieldSubtitle': '副标题（可选）',

  'station.fieldDesc': '描述（可选）',

  'station.fieldCover': '封面 URL（可选）',

  'station.fieldVisibility': '可见性',
  'station.visPrivate': '私密（服务鉴权）',
  'station.visPublic': '公开（免登录收听）',

  'station.save': '保存',
  'station.cancel': '取消',
  'station.back': '返回列表',

  /* 歌单编排 */

  'station.plTracks': '{count} 首',

  /* 节目表网格 */
  'station.schTitle': '节目表',

  'station.schMon': '一',
  'station.schTue': '二',
  'station.schWed': '三',
  'station.schThu': '四',
  'station.schFri': '五',
  'station.schSat': '六',
  'station.schSun': '日',

  'station.schStart': '开始',
  'station.schEnd': '结束',

  'station.schPriority': '优先级',

  'station.schDelete': '删除时段',

} as const;

export const stationEn: Record<keyof typeof stationZh, string> = {
  'station.visibilityBadge': ' ({visibility})',
  'station.oneTrack': '{count} track',
  'station.weekday': '{day}',
  'station.localTimezone': 'Server local time zone',
  'station.issueEmptyQueue': 'Add a playlist or all tracks before broadcasting.',
  'station.issueMissingSource': 'Playlist no longer exists: {name}',
  'station.issueEmptySource': 'No playable tracks in playlist: {name}',
  'station.issueUncoveredTime': 'No program at {day} {time}. Add a regular playlist or complete the schedule.',
  'station.allLibrary':'All tracks', 'station.librarySource':'Built-in',

  'station.priorityInvalid':'Priority must be an integer from 0 to 65535.', 'station.slotNumber':'Slot {number}',

  'station.emptySlots':'Drag a range to add a slot, or add one and edit it directly.',
  'station.nameRequired':'Station name (required)', 'station.visibilityRequired':'Visibility (required)',
  'station.formHint':'Complete the station details. Optional fields may be left blank.', 'station.editInfo':'Edit station details',
  'station.workbenchSubtitle':'Station playlists and weekly programming',
  'station.dragSchedule':'Drag weekly schedule', 'station.newSlot':'New slot',
  'station.systemPlaylists':'System playlists', 'station.broadcastQueue':'Broadcast queue',
  'station.smart':'Smart', 'station.manual':'Manual', 'station.addPlaylist':'Add {name}', 'station.addedPlaylist':'{name} already added',
  'station.queueHint':'Play each regular playlist once in queue order; scheduled items take priority during their time slots.',
  'station.emptyQueue':'Use + on a playlist card above to add a program.', 'station.moveUp':'Move up', 'station.moveDown':'Move down', 'station.removeQueueItem':'Remove {name}',
  'station.playOrder':'Track order', 'station.ordered':'Playlist order', 'station.shuffle':'Shuffle', 'station.regular':'Regular rotation',
  'station.scheduled':'Scheduled',

  'station.gridHint':'One hour per cell · drag to add slots, then save once',
  'station.currentSlot':'Current playlist', 'station.otherSlot':'Other shows',
  'station.timeInvalid':'Choose weekdays and valid, different start/end times (HH:MM).', 'station.timeConflict':'Same-priority slots overlap. Adjust the range or increase its priority.',
  'station.loading':'Loading…',
  'station.publicShort':'Public', 'station.privateShort':'Private',
  'station.configure':'Configure', 'station.configFor':'Configuration for {name}',
  'station.unsaved':'Unsaved changes', 'station.savedPending':'Saved, pending application',
  'station.upToDate':'Configuration applied', 'station.saveConfig':'Save configuration', 'station.applyHint':'Applying may briefly reconnect the stream.',
  'station.apply':'Apply changes', 'station.applied':'Configuration applied',
  'station.copyFailed':'Copy failed. Check clipboard permissions.', 'station.deleteStation':'Delete station "{name}" and its program? System playlists are kept.',
  'station.unsavedHint':'Save your changes, or discard them to continue.', 'station.keepEditing':'Keep editing', 'station.discard':'Discard changes',
  'station.pageTitle': 'Broadcasting',
  'station.pageSubtitle': 'Manage stations, arrange playlists and schedules, and broadcast to listeners',

  'station.new': 'New station',
  'station.empty': 'No stations yet. Create one, arrange playlists & schedules, then go on air.',

  'station.stateOn': 'On air',
  'station.stateOff': 'Stopped',

  'station.play': 'Go on air',
  'station.stop': 'Stop',
  'station.skip': 'Skip',

  'station.del': 'Delete',

  'station.copyHls': 'Copy stream URL',
  'station.copied': 'Copied',

  'station.formTitleNew': 'New station',

  'station.fieldNamePh': 'e.g. "Midnight FM"',
  'station.fieldSubtitle': 'Subtitle (optional)',

  'station.fieldDesc': 'Description (optional)',

  'station.fieldCover': 'Cover URL (optional)',

  'station.fieldVisibility': 'Visibility',
  'station.visPrivate': 'Private (server authentication)',
  'station.visPublic': 'Public (no sign-in required)',

  'station.save': 'Save',
  'station.cancel': 'Cancel',
  'station.back': 'Back to list',

  'station.plTracks': '{count} tracks',

  'station.schTitle': 'Schedule',

  'station.schMon': 'Mon',
  'station.schTue': 'Tue',
  'station.schWed': 'Wed',
  'station.schThu': 'Thu',
  'station.schFri': 'Fri',
  'station.schSat': 'Sat',
  'station.schSun': 'Sun',

  'station.schStart': 'Start',
  'station.schEnd': 'End',

  'station.schPriority': 'Priority',

  'station.schDelete': 'Delete slot',

} as const;
