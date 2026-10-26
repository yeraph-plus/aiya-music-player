<script lang="ts">
  import type { PlaylistSummary } from '../lib/ipc';
  import { artworkUrl } from '../lib/artwork';
  import Button from './ui/Button.svelte';
  import Cover from './ui/Cover.svelte';
  import Dialog from './ui/Dialog.svelte';
  import Field from './ui/Field.svelte';
  import Menu from './ui/Menu.svelte';
  import Toast from './ui/Toast.svelte';
  import { playlistMenuItems, trackMenuItems } from '../lib/menu';
  import {
    ADDED_PRESETS,
    DEFAULT_TIME_FILTER,
    LAST_PLAYED_PRESETS,
    type LastPlayedPreset,
    type AddedPreset,
  } from '../lib/time-filter';
  import type { TrackTimeFilter } from '../lib/time-filter';
  import SegmentedControl from './ui/SegmentedControl.svelte';
  import { library } from '../stores/library.svelte';
  import { list } from '../stores/list.svelte';
  import { nav } from '../stores/nav.svelte';
  import { notify } from '../stores/notify.svelte';
  import { overlay } from '../stores/overlay.svelte';
  import { playlists } from '../stores/playlists.svelte';
  import { player } from '../stores/player.svelte';
  import { selection } from '../stores/selection.svelte';
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
    return trackMenuItems(
      menu.track,
      ui.view,
      {
        play: () => void list.playIndex(menu.index),
        insertNext: () => {
          if (menu.track?.id) player.insertNext(menu.track.id);
        },
        // 批量语义（docs/27 第 6 点）：动作对**整个选择集**生效——右键前
        // LibraryView 已保证目标行在选择集里（不在就单选它），所以这里
        // 统一走 selectionIds()，单选/多选同一条路径，不需要分支。
        enqueue: () => {
          void (async () => {
            const ids = await list.selectionIds();
            if (ids.length === 0) return;
            await player.enqueue(ids);
            notify.showToast(`已加入队列 ${ids.length} 首`);
          })();
        },
        addToPlaylist: () => {
          void (async () => {
            const ids = await list.selectionIds();
            if (ids.length === 0) return;
            overlay.dialog = { kind: 'addToPlaylist', ids };
          })();
        },
        filterByArtist: () => {
          // B4-3b 升级：从"列表 + 筛选 chip"改成直达艺人详情页（docs/24 §2 B4）
          const id = menu.track?.artistId;
          if (!id) return;
          nav.go({ kind: 'artist', id });
        },
        filterByAlbum: () => {
          const id = menu.track?.albumId;
          if (!id) return;
          nav.go({ kind: 'album', id });
        },
        reveal: () => void revealTrack(menu.track?.id),
        remove: () => {
          // 歌单视图里是"从歌单移除"（按下标，倒序防位移），
          // 曲库里是"从曲库移除"（按 ID 走确认对话框）
          if (ui.view.kind === 'playlist') {
            void playlists.removeItemsAt(selection.sortedIndices());
          } else {
            void (async () => {
              const ids = await list.selectionIds();
              if (ids.length === 0) return;
              overlay.dialog = { kind: 'removeTracks', ids };
            })();
          }
        },
      },
      menu.multi,
    );
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

  /* ---------------- 五个对话框的状态与提交（B6 删壳后收拢到这里） ---------------- */

  /** 两个输入对话框共用一个文本状态；打开/切换时按语义初始化 */
  let promptText = $state('');
  $effect(() => {
    const d = overlay.dialog;
    if (d?.kind === 'createPlaylist') promptText = '';
    else if (d?.kind === 'renamePlaylist') promptText = d.playlist.name;
  });

  /* ---------------- 筛选器模态框（docs/27 第 2/3 点） ---------------- */

  /** 草稿：打开时从 list.timeFilter 复制，应用才写回（取消不落库） */
  let filterDraft = $state<TrackTimeFilter>({ ...DEFAULT_TIME_FILTER });
  $effect(() => {
    if (overlay.dialog?.kind === 'trackFilter') filterDraft = { ...list.timeFilter };
  });

  let createInput = $state<HTMLInputElement | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);
  /** 危险对话框的焦点默认落「取消」——回车误确认的代价不可逆（见模板头注释） */
  let deleteCancelEl = $state<HTMLButtonElement | null>(null);
  let removeCancelEl = $state<HTMLButtonElement | null>(null);

  function submitPrompt(): void {
    const value = promptText.trim();
    if (!value) return;
    const d = overlay.dialog;
    if (d?.kind === 'createPlaylist') {
      const seed = d.seedIds;
      overlay.closeDialog();
      void playlists.create(value, seed);
    } else if (d?.kind === 'renamePlaylist') {
      const playlist = d.playlist;
      overlay.closeDialog();
      void playlists.rename(playlist, value);
    }
  }

  function submitDelete(): void {
    if (overlay.dialog?.kind !== 'deletePlaylist') return;
    const playlist = overlay.dialog.playlist;
    overlay.closeDialog();
    void playlists.remove(playlist);
  }

  function submitRemoveTracks(): void {
    if (overlay.dialog?.kind !== 'removeTracks') return;
    const ids = overlay.dialog.ids;
    overlay.closeDialog();
    void list.removeTracks(ids);
  }

  function submitAddTo(playlistId: string): void {
    if (overlay.dialog?.kind !== 'addToPlaylist') return;
    const ids = overlay.dialog.ids;
    overlay.closeDialog();
    void playlists.addTracks(playlistId, ids);
  }
</script>

{#if notify.toast}
  <Toast message={notify.toast} />
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
  <!--
    三个对话框（B6 删壳后直接内联，五个分支同住一处的「浮层宿主」才完整）：

    - **焦点默认落「取消」**（deletePlaylist/removeTracks）：危险操作的默认动作
      必须是"什么都不做"，回车误确认的代价不可逆。靠 autofocus 指定元素实现，
      不拦 Enter——拦截按键会和浏览器原生激活打架。
    - **输入框 text 在对话框打开时初始化**（见 script 里的 effect）：value 只在
      打开那一刻有意义，否则歌单列表刷新会把用户正在输的字覆盖掉。
    - 「添加到播放列表」**不用 Menu**：选歌单是"看得到每个歌单有几首"的浏览
      行为，菜单是"我已经知道要点哪一项"的快捷路径，混用两头不像。
  -->
  <Dialog title="新建播放列表" description="给歌单起个名字" onclose={() => overlay.closeDialog()}>
    <div class="prompt-body">
      <Field
        label="名称"
        bind:ref={createInput}
        value={promptText}
        maxlength={80}
        oninput={(next) => (promptText = next)}
        onenter={submitPrompt}
      />
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="primary" disabled={!promptText.trim()} onclick={submitPrompt}>创建</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'renamePlaylist'}
  <Dialog
    title="重命名播放列表"
    onclose={() => overlay.closeDialog()}
    autofocus={() => {
      renameInput?.focus();
      // 重命名场景是「改已有名字」，全选后直接输入比让用户先删更快
      renameInput?.select();
    }}
  >
    <div class="prompt-body">
      <Field
        label="名称"
        bind:ref={renameInput}
        value={promptText}
        maxlength={80}
        oninput={(next) => (promptText = next)}
        onenter={submitPrompt}
      />
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="primary" disabled={!promptText.trim()} onclick={submitPrompt}>保存</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'deletePlaylist'}
  <Dialog
    title="删除播放列表"
    description={`「${overlay.dialog.playlist.name}」会被删除，曲库里的歌曲不受影响。`}
    onclose={() => overlay.closeDialog()}
    autofocus={() => deleteCancelEl?.focus()}
  >
    {#snippet actions()}
      <Button bind:ref={deleteCancelEl} onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="danger" onclick={submitDelete}>删除</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'removeTracks'}
  <Dialog
    title="从曲库移除"
    description={`将从曲库移除 ${overlay.dialog.ids.length} 首曲目。磁盘上的文件不会被删除，下次扫描会重新导入。`}
    onclose={() => overlay.closeDialog()}
    autofocus={() => removeCancelEl?.focus()}
  >
    {#snippet actions()}
      <Button bind:ref={removeCancelEl} onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="danger" onclick={submitRemoveTracks}>移除</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'trackFilter'}
  <!--
    筛选器（docs/27 第 2/3 点）：入库时间 / 上次播放两组预设档。
    后续 beets 标签字段按同样的「档位分段控件 → 查询参数」模式加在这里。
  -->
  <Dialog
    title="筛选曲目"
    description="按入库时间或上次播放过滤当前列表"
    onclose={() => overlay.closeDialog()}
  >
    <div class="filter-body">
      <div class="filter-row">
        <div class="filter-label">入库时间</div>
        <SegmentedControl
          label="入库时间"
          value={filterDraft.added}
          options={ADDED_PRESETS}
          onchange={(value) => (filterDraft = { ...filterDraft, added: value as AddedPreset })}
        />
      </div>
      <div class="filter-row">
        <div class="filter-label">上次播放</div>
        <SegmentedControl
          label="上次播放"
          value={filterDraft.lastPlayed}
          options={LAST_PLAYED_PRESETS}
          onchange={(value) =>
            (filterDraft = { ...filterDraft, lastPlayed: value as LastPlayedPreset })}
        />
      </div>
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>取消</Button>
      <Button
        variant="primary"
        onclick={() => {
          overlay.closeDialog();
          list.setTimeFilter(filterDraft);
        }}
      >
        应用
      </Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'addToPlaylist'}
  <!--
    极简歌单选择器（docs/25 用户反馈）：无大标题、无关闭键、无底部按钮——
    顶部只留一行**次要文本**说明用途，主体就一份歌单列表，**点行即添加
    并关闭**；没有歌单就一行空态文案。关闭走 Esc / 点遮罩。
    不用 Menu：选歌单是"看得到每个歌单有几首"的浏览行为，菜单是
    "我已经知道要点哪一项"的快捷路径，混用两头不像。
  -->
  <Dialog
    label="添加到播放列表"
    description="添加到播放列表"
    width="md"
    onclose={() => overlay.closeDialog()}
  >
    <div class="pick-list">
      {#each playlists.items as playlist (playlist.id)}
        <button class="pick-row" onclick={() => submitAddTo(playlist.id)}>
          <Cover src={playlist.artworkId ? artworkUrl(playlist.artworkId, 96) : null} size="sm" />
          <span class="pick-name">{playlist.name}</span>
          <span class="pick-count">{playlist.trackCount} 首</span>
        </button>
      {:else}
        <p class="pick-empty">还没有播放列表</p>
      {/each}
    </div>
  </Dialog>
{/if}

<style>
  /* 输入对话框与歌单选择器的主体（原 PromptDialog / PlaylistPicker 的 scoped 样式，
     B6 删壳后随内联迁来） */
  .prompt-body {
    padding: 14px 16px 2px;
  }

  /* 筛选器模态框：两组档位分段控件 */
  .filter-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 16px 4px;
  }

  .filter-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .filter-label {
    flex: none;
    font-size: 12.5px;
    color: var(--text-2);
  }

  .pick-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
  }

  .pick-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--text-1);
    font-size: 12.5px;
  }

  .pick-row:hover {
    background: var(--surface-hover);
  }

  .pick-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pick-count {
    flex: none;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .pick-empty {
    margin: 0;
    padding: 18px 10px;
    font-size: 12.5px;
    color: var(--text-2);
    text-align: center;
  }
</style>
