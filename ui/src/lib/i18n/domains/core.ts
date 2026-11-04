/** core 域：format.ts 的单位/状态、首页时段问候（C0 基建自带）。
 *  键名按域前缀命名；en 表 `Record<keyof typeof zh, string>` 保证键位奇偶。 */

export const coreZh = {
  'format.duration.zero': '0 分钟',
  'format.duration.minutes': '{m} 分钟',
  'format.duration.h-min': '{h} 小时 {m} 分钟',
  'format.repeat.one': '单曲循环',
  'format.repeat.all': '列表循环',
  'format.repeat.off': '顺序播放',
  'format.status.loading': '正在载入…',
  'format.status.playing': '正在播放',
  'format.status.paused': '已暂停',
  'format.status.error': '播放出错',
  'home.greeting.night': '夜深了，今天听点什么？',
  'home.greeting.morning': '早上好，今天听点什么？',
  'home.greeting.noon': '中午好，今天听点什么？',
  'home.greeting.afternoon': '下午好，今天听点什么？',
  'home.greeting.evening': '晚上好，今天听点什么？',
} as const;

export const coreEn: Record<keyof typeof coreZh, string> = {
  'format.duration.zero': '0 min',
  'format.duration.minutes': '{m} min',
  'format.duration.h-min': '{h} h {m} min',
  'format.repeat.one': 'Repeat one',
  'format.repeat.all': 'Repeat all',
  'format.repeat.off': 'Play in order',
  'format.status.loading': 'Loading…',
  'format.status.playing': 'Now playing',
  'format.status.paused': 'Paused',
  'format.status.error': 'Playback error',
  'home.greeting.night': "It's late!",
  'home.greeting.morning': 'Good morning!',
  'home.greeting.noon': 'Hello!',
  'home.greeting.afternoon': 'Good afternoon!',
  'home.greeting.evening': 'Good evening!',
};
