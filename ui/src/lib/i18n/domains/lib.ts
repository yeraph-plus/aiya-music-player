/** lib 域：lib/ 纯函数与 stores/ 业务侧渲染到 UI 的文案，按来源文件分组：
 *  menu（右键菜单）/ smart（动态歌单校验）/ liveMessage（直播状态与流来源）/
 *  audioFormat（音频规格悬停提示）/ drop（拖放蒙版与吐司）。
 *  zh 值与界面现行中文逐字一致（e2e 断言中文文案）；en 按术语表新写。
 *  en 表 `Record<keyof typeof zh, string>` 保证键位奇偶。 */

export const libZh = {
  /* menu.ts：曲目右键菜单 */
  'lib.menu.play': '播放',
  'lib.menu.playNext': '下一首播放',
  'lib.menu.enqueue': '加入播放队列',
  'lib.menu.addToPlaylist': '添加到播放列表…',
  'lib.menu.viewArtist': '查看艺人',
  'lib.menu.viewAlbum': '查看专辑',
  'lib.menu.reveal': '在文件夹中显示',
  'lib.menu.removeFromPlaylist': '从播放列表移除',
  'lib.menu.removeFromLibrary': '从曲库移除',
  'lib.menu.rename': '重命名',
  'lib.menu.editRules': '编辑规则',
  'lib.menu.moveUp': '上移',
  'lib.menu.moveDown': '下移',
  'lib.menu.clear': '清空',
  'lib.menu.delete': '删除',

  /* smart.ts：动态歌单规则校验（字段名经 validateRule 拼进错误文案） */
  'lib.smart.playCount': '播放次数',
  'lib.smart.ruleTextRequired': '{field}需填写内容',
  'lib.smart.ruleNumberRequired': '{field}需填数字',
  'lib.smart.ruleOpInvalid': '{field}操作符无效',

  /* live.ts + format.ts：直播三态徽章（复用 shell.live.*）与流来源短句 */
  'lib.liveMessage.localPlaylist': '本地播放列表',

  /* audio-format.ts：audioTooltip 的单位与三态 */
  'lib.audioFormat.channels': '{n} 声道',
  'lib.audioFormat.lossless': '无损',
  'lib.audioFormat.lossy': '有损',
  'lib.audioFormat.unknown': '未知',
  'lib.audioFormat.avgBitrate': '平均码率 {kbps} kbps',

  /* stores/drop.svelte.ts：拖放蒙版预告与落点吐司 */
  'lib.drop.hintPlayable': '{n} 项可播放',
  'lib.drop.hintPlaylists': '{n} 个歌单会展开',
  'lib.drop.hintFolders': '{n} 个文件夹跳过（走设置 → 曲库导入）',
  'lib.drop.hintUnsupported': '{n} 项格式不支持',
  'lib.drop.hintEmpty': '没有可识别的内容',
  'lib.drop.hintItems': '{n} 项',
  'lib.drop.reasonsSep': '；',
  'lib.drop.reasonsMore': '等 {n} 项',
  'lib.drop.openFailed': '无法播放：{reasons}',
  'lib.drop.skippedSuffix': '，跳过 {reasons}',
  'lib.drop.opened': '已打开 {n} 项{skipped}',
} as const;

export const libEn: Record<keyof typeof libZh, string> = {
  'lib.menu.play': 'Play',
  'lib.menu.playNext': 'Play next',
  'lib.menu.enqueue': 'Add to queue',
  'lib.menu.addToPlaylist': 'Add to playlist…',
  'lib.menu.viewArtist': 'View artist',
  'lib.menu.viewAlbum': 'View album',
  'lib.menu.reveal': 'Show in folder',
  'lib.menu.removeFromPlaylist': 'Remove from playlist',
  'lib.menu.removeFromLibrary': 'Remove from library',
  'lib.menu.rename': 'Rename',
  'lib.menu.editRules': 'Edit rules',
  'lib.menu.moveUp': 'Move up',
  'lib.menu.moveDown': 'Move down',
  'lib.menu.clear': 'Clear',
  'lib.menu.delete': 'Delete',

  'lib.smart.playCount': 'Play count',
  'lib.smart.ruleTextRequired': '{field} needs a value',
  'lib.smart.ruleNumberRequired': '{field} needs a number',
  'lib.smart.ruleOpInvalid': 'invalid operator for {field}',

  'lib.liveMessage.localPlaylist': 'Local playlist',

  'lib.audioFormat.channels': '{n} ch',
  'lib.audioFormat.lossless': 'lossless',
  'lib.audioFormat.lossy': 'lossy',
  'lib.audioFormat.unknown': 'unknown',
  'lib.audioFormat.avgBitrate': 'avg bitrate {kbps} kbps',

  'lib.drop.hintPlayable': '{n} playable',
  'lib.drop.hintPlaylists': '{n} playlists will expand',
  'lib.drop.hintFolders': '{n} folders skipped (use Settings → Library to import)',
  'lib.drop.hintUnsupported': '{n} unsupported',
  'lib.drop.hintEmpty': 'nothing recognizable',
  'lib.drop.hintItems': '{n} items',
  'lib.drop.reasonsSep': '; ',
  'lib.drop.reasonsMore': 'and {n} more',
  'lib.drop.openFailed': 'failed to play: {reasons}',
  'lib.drop.skippedSuffix': ', skipped: {reasons}',
  'lib.drop.opened': 'Opened {n} items{skipped}',
};
