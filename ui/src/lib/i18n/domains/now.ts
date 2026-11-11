/** now 域：正在播放簇（NowPlayingView / QueuePanel / LyricsPanel）与首页
 *  （HomeView）剩余文案。频谱组件（NowPlayingSpectrum）无用户面字符串。
 *  复用既有键：播放队列=shell.drawer.queue、清空=library.clear、{n} 首=
 *  common.trackCount、正在播放=format.status.playing、直播=shell.live.on、
 *  未在播放=shell.player.notPlaying、未知艺人=shell.player.unknownArtist、
 *  查看专辑/歌手=tracks.viewAlbum/tracks.viewArtist、最近播放/最近添加=
 *  library.tabRecent/library.tabAdded、歌单=shell.sidebar.playlists。
 *  zh 值与界面现行中文逐字一致（e2e 断言中文文案）；en 按术语表新写
 *  （常听=Most Played、累计播放=total plays、收听时长=listening time、
 *  位置缺失=missing locations）。
 *  en 表 `Record<keyof typeof zh, string>` 保证键位奇偶。 */

export const nowZh = {
  /* HomeView：歌单区标题行、歌单卡与空态 */
  'now.home.playlistCount': '{n} 个',
  'now.home.missingLocationsCta': '{n} 个位置缺失，去处理',
  'now.home.playPlaylistTip': '播放「{name}」',
  'now.home.playPlaylistAria': '播放 {name}',
  'now.home.playlistEmpty': '从曲目右键菜单或批量操作创建歌单。',

  /* HomeView：播放统计区 */
  'now.home.statsAria': '播放统计',
  'now.home.statTotalPlays': '累计播放',
  'now.home.statListeningTime': '收听时长',
  'now.home.playsValue': '{n} 次',

  /* HomeView：双表与最近添加的空态（标题复用 library.tabRecent/tabAdded） */
  'now.home.mostPlayed': '常听',
  'now.home.mostPlayedEmpty': '播放次数积累后，这里会长出你的常听曲目。',
  'now.home.recentPlayedEmpty': '播放后，这里会留下最近的轨迹。',
  'now.home.recentAddedEmpty': '扫描入库后，最新的曲目会出现在这里。',

  /* NowPlayingView：封面 alt 与空态（未在播放/未知艺人/查看专辑/查看歌手复用
     shell.player.* 与 tracks.*，zh 完全同文） */
  'now.np.coverAlt': '《{title}》封面',
  'now.np.emptyHint': '从推荐页或歌单选择一首歌开始',

  /* QueuePanel：头部、空态与行内标记（临时项的直播标记复用 shell.live.on） */
  'now.queue.close': '关闭队列',
  'now.queue.emptyTitle': '队列是空的',
  'now.queue.emptyHint': '右键曲目可加入队列',
  'now.queue.remove': '从队列移除',
  'now.queue.transientFile': '文件',
  'now.queue.loadingMeta': '载入中…',

  /* LyricsPanel：容器 aria、查找/无词空态、纯文本与同步歌词的 aria、行 seek 提示 */
  'now.lyrics.aria': '歌词',
  'now.lyrics.searching': '正在查找歌词…',
  'now.lyrics.currentTrack': '当前曲目',
  'now.lyrics.notFoundTitle': '《{title}》没有歌词',
  'now.lyrics.notFoundHint': '在同目录放一个同名的 .lrc 文件即可显示',
  'now.lyrics.plainAria': '歌词（纯文本）：《{title}》',
  'now.lyrics.syncedAria': '同步歌词：《{title}》，点击或回车某一行跳转到该句',
  'now.lyrics.lineSeekTip': '跳到这一句',
} as const;

export const nowEn: Record<keyof typeof nowZh, string> = {
  'now.home.playlistCount': '{n}',
  'now.home.missingLocationsCta': '{n} missing locations — review them',
  'now.home.playPlaylistTip': 'Play "{name}"',
  'now.home.playPlaylistAria': 'Play {name}',
  'now.home.playlistEmpty': 'Create playlists from the track right-click menu or batch actions.',

  'now.home.statsAria': 'Play statistics',
  'now.home.statTotalPlays': 'Total plays',
  'now.home.statListeningTime': 'Listening time',
  'now.home.playsValue': '{n} plays',

  'now.home.mostPlayed': 'Most Played',
  'now.home.mostPlayedEmpty': 'Your most played tracks will show up here as play counts build up.',
  'now.home.recentPlayedEmpty': 'Tracks you play will leave a trail here.',
  'now.home.recentAddedEmpty': 'Newly imported tracks will show up here after a scan.',

  'now.np.coverAlt': 'Cover art for {title}',
  'now.np.emptyHint': 'Pick a track from Home or a playlist to start',

  'now.queue.close': 'Close queue',
  'now.queue.emptyTitle': 'The queue is empty',
  'now.queue.emptyHint': 'Right-click a track to add it to the queue',
  'now.queue.remove': 'Remove from queue',
  'now.queue.transientFile': 'File',
  'now.queue.loadingMeta': 'Loading…',

  'now.lyrics.aria': 'Lyrics',
  'now.lyrics.searching': 'Looking up lyrics…',
  'now.lyrics.currentTrack': 'current track',
  'now.lyrics.notFoundTitle': '"{title}" has no lyrics',
  'now.lyrics.notFoundHint': 'Put a .lrc file with the same name in the same folder to display lyrics',
  'now.lyrics.plainAria': 'Lyrics (plain text): "{title}"',
  'now.lyrics.syncedAria': 'Synced lyrics: "{title}" — click a line or press Enter to jump to it',
  'now.lyrics.lineSeekTip': 'Seek to this line',
};
