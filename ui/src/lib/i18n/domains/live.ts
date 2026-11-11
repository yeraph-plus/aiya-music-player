/** live 域：直播页（LiveView）的页面文案——卡组、添加流弹窗、开播吐司。
 *  直播三态徽章（直播/连接中/已断开）由 lib/live.ts 的 `livePhase` 渲染，
 *  复用 shell 域的 shell.live.*（播放条与直播页同一口径），不在本域重复。
 *  zh 值与界面现行中文逐字一致（e2e 断言中文文案）；en 按术语表新写。
 *  en 表 `Record<keyof typeof zh, string>` 保证键位奇偶。 */

export const liveZh = {
  /* 页头 */
  'live.title': '直播',
  'live.subtitle': '收听网络电台与 HLS 直播流',
  'live.addStream': '添加流',
  'live.openFile': '打开文件…',

  /* 卡组：卡面、箭头、开播引导 */
  'live.cardPlayAria': '播放 {name}',
  'live.removeRecent': '从最近打开中移除',
  'live.prevStream': '上一条流',
  'live.nextStream': '下一条流',
  'live.clickToPlay': '点击开播',

  /* 添加流弹窗 */
  'live.fieldUrl': 'm3u8 地址',
  'live.fieldName': '名称（可选）',
  'live.fieldNamePh': '留空则显示主机名',
  'live.fieldSubtitle': '副标题（可选）',
  'live.fieldSubtitlePh': '悬浮层流名下的一行小字，如「Chill · 24h」',
  'live.fieldDesc': '描述（可选）',
  'live.fieldDescPh': '悬浮层的说明文字，过长会截断',
  'live.addConfirm': '添加',
  'live.errorHttpOnly': '仅支持 http(s) 地址',

  /* 开流与打开本地播放列表的吐司 */
  'live.toastExists': '已在列表中：{name}',
  'live.toastAdded': '已添加流',
  'live.toastConnecting': '正在连接 {name}',
  'live.toastOpenFailed': '打不开：{reasons}',
  'live.toastPlaylistOpened': '已打开播放列表',
} as const;

export const liveEn: Record<keyof typeof liveZh, string> = {
  'live.title': 'Live',
  'live.subtitle': 'Listen to internet radio and HLS live streams',
  'live.addStream': 'Add stream',
  'live.openFile': 'Open file…',

  'live.cardPlayAria': 'Play {name}',
  'live.removeRecent': 'Remove from recents',
  'live.prevStream': 'Previous stream',
  'live.nextStream': 'Next stream',
  'live.clickToPlay': 'Click to play',

  'live.fieldUrl': 'm3u8 URL',
  'live.fieldName': 'Name (optional)',
  'live.fieldNamePh': 'leave empty to show the host name',
  'live.fieldSubtitle': 'Subtitle (optional)',
  'live.fieldSubtitlePh': 'a small line under the stream name, e.g. "Chill · 24h"',
  'live.fieldDesc': 'Description (optional)',
  'live.fieldDescPh': 'description text on the hover layer; truncated when too long',
  'live.addConfirm': 'Add',
  'live.errorHttpOnly': 'only http(s) URLs are supported',

  'live.toastExists': 'already in the list: {name}',
  'live.toastAdded': 'Stream added',
  'live.toastConnecting': 'Connecting to {name}',
  'live.toastOpenFailed': 'failed to open: {reasons}',
  'live.toastPlaylistOpened': 'Playlist opened',
};
