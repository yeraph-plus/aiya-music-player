<script lang="ts">
  import ConfirmDialog from './ConfirmDialog.svelte';
  import Menu from './ui/Menu.svelte';
  import PlaylistPicker from './PlaylistPicker.svelte';
  import PromptDialog from './PromptDialog.svelte';
  import { playlistMenuItems, trackMenuItems } from '../lib/menu';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { player } from '../stores/player.svelte';
  import { ui } from '../stores/ui.svelte';

  /**
   * 浮层宿主：轻提示 + 两个右键菜单 + 五个对话框 + 设置面板。
   *
   * 为什么单独一个组件（批次 3）：这些全是**页面级浮层**，不参与主区布局
   * —— 挂在 `AppShell` 里会被 `.app` 网格的 `overflow: hidden` 裁掉，
   * 而挂在 `App` 里就是 140 行模板跟着 App 一起变胖。
   *
   * 它自己读 store、自己调 store 的动作，**不收任何 prop**：菜单项要的
   * 上下文（当前视图、选择集）都在 store 里。唯一"取数"是 revealTrack
   * （在系统文件管理器里定位），失败转成错误条。
   */

  /** 在资源管理器中定位；失败把 reason 显示成错误条（这是用户显式动作） */
  async function revealTrack(trackId: string | null | undefined): Promise<void> {
    if (!trackId) return;
    const result = await library.revealTrack(trackId);
    if (result && !result.revealed) notify.showError(result.reason ?? '无法定位文件');
  }

  function trackItems(menu: NonNullable<typeof overlay.trackMenu>) {
    return trackMenuItems(menu.track, ui.view, {
      play: () => void list.playIndex(menu.index),
      insertNext: () => {
        if (menu.track?.id) player.insertNext(menu.track.id);
      },
      enqueue: () => {
        if (menu.track?.id) void player.enqueue([menu.track.id]);
      },
      addToPlaylist: () => {
        if (menu.track?.id) overlay.dialog = { kind: 'addToPlaylist', ids: [menu.track.id] };
      },
      filterByArtist: () => {
        const id = menu.track?.artistId;
        if (!id) return;
        ui.artistId = id;
        void list.reload();
      },
      filterByAlbum: () => {
        const id = menu.track?.albumId;
        if (!id) return;
        ui.albumId = id;
        void list.reload();
      },
      reveal: () => void revealTrack(menu.track?.id),
      remove: () => {
        // 歌单视图里是"从歌单移除"，曲库里是"从曲库移除"——同一个菜单项两种语义
        if (ui.view.kind === 'playlist') void playlists.removeItemAt(menu.index);
        else if (menu.track?.id) overlay.dialog = { kind: 'removeTracks', ids: [menu.track.id] };
      },
    });
  }

  function playlistItems(menu: NonNullable<typeof overlay.playlistMenu>) {
    const index = playlists.items.findIndex((item) => item.id === menu.playlist.id);
    return playlistMenuItems(
      menu.playlist,
      { index, total: playlists.items.length },
      {
        play: () => void playlists.play(menu.playlist),
        rename: () => {
          overlay.dialog = { kind: 'renamePlaylist', playlist: menu.playlist };
        },
        moveUp: () => void playlists.moveOrder(menu.playlist, -1),
        moveDown: () => void playlists.moveOrder(menu.playlist, 1),
        clear: () => void playlists.clear(menu.playlist),
        remove: () => {
          overlay.dialog = { kind: 'deletePlaylist', playlist: menu.playlist };
        },
      },
    );
  }
</script>

{#if notify.toast}
  <div class="toast" aria-live="polite">{notify.toast}</div>
{/if}

{#if overlay.trackMenu}
  {@const menu = overlay.trackMenu}
  <Menu
    x={menu.x}
    y={menu.y}
    onclose={() => (overlay.trackMenu = null)}
    items={trackItems(menu)}
  />
{/if}

{#if overlay.playlistMenu}
  {@const menu = overlay.playlistMenu}
  <Menu
    x={menu.x}
    y={menu.y}
    onclose={() => (overlay.playlistMenu = null)}
    items={playlistItems(menu)}
  />
{/if}

{#if overlay.dialog?.kind === 'createPlaylist'}
  <PromptDialog
    title="新建播放列表"
    label="名称"
    confirmText="创建"
    onconfirm={(name) => {
      const seed = overlay.dialog?.kind === 'createPlaylist' ? overlay.dialog.seedIds : undefined;
      overlay.closeDialog();
      void playlists.create(name, seed);
    }}
    oncancel={() => overlay.closeDialog()}
  />
{:else if overlay.dialog?.kind === 'renamePlaylist'}
  <PromptDialog
    title="重命名播放列表"
    label="名称"
    value={overlay.dialog.playlist.name}
    confirmText="保存"
    onconfirm={(name) => {
      if (overlay.dialog?.kind !== 'renamePlaylist') return;
      const playlist = overlay.dialog.playlist;
      overlay.closeDialog();
      void playlists.rename(playlist, name);
    }}
    oncancel={() => overlay.closeDialog()}
  />
{:else if overlay.dialog?.kind === 'deletePlaylist'}
  <ConfirmDialog
    title="删除播放列表"
    message={`「${overlay.dialog.playlist.name}」会被删除，曲库里的歌曲不受影响。`}
    confirmText="删除"
    danger
    onconfirm={() => {
      if (overlay.dialog?.kind !== 'deletePlaylist') return;
      const playlist = overlay.dialog.playlist;
      overlay.closeDialog();
      void playlists.remove(playlist);
    }}
    oncancel={() => overlay.closeDialog()}
  />
{:else if overlay.dialog?.kind === 'removeTracks'}
  <ConfirmDialog
    title="从曲库移除"
    message={`将从曲库移除 ${overlay.dialog.ids.length} 首曲目。磁盘上的文件不会被删除，下次扫描会重新导入。`}
    confirmText="移除"
    danger
    onconfirm={() => {
      if (overlay.dialog?.kind !== 'removeTracks') return;
      const ids = overlay.dialog.ids;
      overlay.closeDialog();
      void list.removeTracks(ids);
    }}
    oncancel={() => overlay.closeDialog()}
  />
{:else if overlay.dialog?.kind === 'addToPlaylist'}
  <PlaylistPicker
    title="添加到播放列表"
    count={overlay.dialog.ids.length}
    playlists={playlists.items}
    onpick={(playlistId) => {
      if (overlay.dialog?.kind !== 'addToPlaylist') return;
      const ids = overlay.dialog.ids;
      overlay.closeDialog();
      void playlists.addTracks(playlistId, ids);
    }}
    oncreate={() => {
      if (overlay.dialog?.kind !== 'addToPlaylist') return;
      const ids = overlay.dialog.ids;
      overlay.dialog = { kind: 'createPlaylist', seedIds: ids };
    }}
    onclose={() => overlay.closeDialog()}
  />
{/if}
