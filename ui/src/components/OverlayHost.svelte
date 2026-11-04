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
    COND_MODES,
    COND_MODE_ORDER,
    validateRule,
    condOfMode,
    isEmptyCond,
    modeOfCond,
    type CondMode,
    type SmartRule,
  } from '../lib/smart';
  import type { FilterCond } from '../lib/time-filter';
  import Select from './ui/Select.svelte';
  import Switch from './ui/Switch.svelte';
  import { ipc } from '../lib/ipc';
  import { t, type Msg } from '../lib/i18n.svelte';
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
   * 浮层宿主：轻提示、两个右键菜单、五个对话框。
   *
   * 独立组件的原因：这些均为页面级浮层，不参与主区布局。挂在 `AppShell`
   * 内会被 `.app` 网格的 `overflow: hidden` 裁切；挂在 `App` 内则使 App 的
   * 模板随浮层数量增长。
   *
   * 组件自身读取 store 并调用 store 动作，不接收 prop：菜单所需的上下文
   * （当前视图、选择集）均在 store 中。唯一的外部取数动作是 `revealTrack`，
   * 失败时转为错误提示。
   */

  /** 在系统文件管理器中定位文件；失败时把 reason 显示为错误提示（用户显式动作） */
  async function revealTrack(trackId: string | null | undefined): Promise<void> {
    if (!trackId) return;
    const result = await library.revealTrack(trackId);
    if (result && !result.revealed) notify.showError(result.reason ?? t('shell.overlay.revealFailed'));
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
        // 批量语义：动作作用于整个选择集。右键前 LibraryView
        // 已确保目标行属于选择集（不属于则改为单选该行），因此统一调用
        // selectionIds()，单选与多选走同一路径，无需分支。
        enqueue: () => {
          void (async () => {
            const ids = await list.selectionIds();
            if (ids.length === 0) return;
            await player.enqueue(ids);
            notify.showToast(t('shell.overlay.addedToQueue', { n: ids.length }));
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
          // 直达艺人详情页
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
          // 歌单视图为"从歌单移除"（按下标，倒序执行以避免位移），
          // 曲库视图为"从曲库移除"（按 ID 走确认对话框）
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
      // 详情页上"查看自己"没有意义
      ui.page === 'album' ? 'album' : ui.page === 'artist' ? 'artist' : null,
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
          overlay.dialog = { kind: 'playlistEdit', playlist: menu.playlist };
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

  /* 歌单模态框：创建与编辑共用创建与编辑只差标题与初始值；引入动态歌单后编辑还需携带规则编辑器，两个壳合并为一个。规则草稿在打开时从
     playlist 快照，提交时才写回 （取消不落库），与筛选器一致。 */

  /** 与 Rust smart.rs 同语义的兜底解析（坏 JSON 当空规则，不炸模态框） */
  function parseRuleJson(raw: string | null | undefined): SmartRule {
    if (!raw) return { all: [] };
    try {
      const v = JSON.parse(raw) as Partial<SmartRule>;
      return {
        all: Array.isArray(v.all) ? v.all : [],
        // sort/direction 是兼容字段（编辑器不再暴露，视图列头接管），读旧值
        // 只是为了 parse 不丢字段；保存时统一写 title/asc
        sort: v.sort ?? 'title',
        direction: v.direction ?? 'asc',
      };
    } catch {
      return { all: [] };
    }
  }

  let editName = $state('');
  let editKind = $state<'manual' | 'smart'>('manual');
  let editRule = $state<SmartRule>({ all: [], sort: 'title', direction: 'asc' });
  let nameInput = $state<HTMLInputElement | null>(null);

  /** 曲库流派下拉（library store 带缓存；拉不到就只剩"当前值"一项，不炸） */
  let editGenres = $state<string[]>([]);

  $effect(() => {
    const d = overlay.dialog;
    if (d?.kind === 'playlistEdit') {
      editName = d.playlist?.name ?? '';
      editKind = d.playlist ? (d.playlist.kind === 'smart' ? 'smart' : 'manual') : 'manual';
      editRule = parseRuleJson(d.playlist?.ruleJson);
    }
  });

  // 流派下拉独立成 effect：主 effect 若读 editKind，开关一开就会重跑并把
  // 草稿重置回打开时的快照（点"动态歌单"瞬间被打回手动——实测踩过）。
  // 这里读 editKind 是有意的：切到 smart 才拉，重跑无害（store 有缓存）。
  $effect(() => {
    if (overlay.dialog?.kind === 'playlistEdit' && editKind === 'smart') {
      void library
        .genres()
        .then((g) => (editGenres = g))
        .catch(() => (editGenres = []));
    }
  });

  /** 条件模式 → i18n 键：lib/smart.ts 的 COND_MODES 文案渲染进本组件模板，
   *  双语在 shell 域表维护（zh 值与 COND_MODES 逐字一致，e2e 守护）。 */
  const COND_MODE_LABEL: Record<CondMode, Msg> = {
    genre: 'shell.smart.cond.genre.label',
    artist: 'shell.smart.cond.artist.label',
    album: 'shell.smart.cond.album.label',
    yearFrom: 'shell.smart.cond.yearFrom.label',
    yearTo: 'shell.smart.cond.yearTo.label',
    playsMin: 'shell.smart.cond.playsMin.label',
    playsMax: 'shell.smart.cond.playsMax.label',
  };
  const COND_MODE_PLACEHOLDER: Record<CondMode, Msg> = {
    genre: 'shell.smart.cond.genre.placeholder',
    artist: 'shell.smart.cond.artist.placeholder',
    album: 'shell.smart.cond.album.placeholder',
    yearFrom: 'shell.smart.cond.yearFrom.placeholder',
    yearTo: 'shell.smart.cond.yearTo.placeholder',
    playsMin: 'shell.smart.cond.playsMin.placeholder',
    playsMax: 'shell.smart.cond.playsMax.placeholder',
  };

  function addCond(): void {
    // 新条件默认流派——这是用户最常用的入口
    editRule.all = [...editRule.all, condOfMode('genre')];
  }

  function removeCond(index: number): void {
    editRule.all = editRule.all.filter((_, i) => i !== index);
  }

  /**
   * 换条件模式。值能保留就保留：新旧控件同为文本（流派→艺人）或同为数字
   * （年份下限→上限）时，用户已输的内容不丢；类型变了才重置。
   */
  function setCondMode(index: number, mode: CondMode): void {
    const cur = editRule.all[index];
    const spec = COND_MODES[mode];
    const sameType = (typeof cur.value === 'number') === (spec.control === 'number');
    editRule.all[index] = sameType
      ? { ...cur, field: spec.field, op: spec.op }
      : condOfMode(mode);
  }

  /** 流派下拉选项：当前值不在曲库流派里时插到最前。
   *  规则值若不在候选项中被丢弃，会静默改掉用户已保存的筛选条件。 */
  function genreOptions(current: string): string[] {
    if (!current) return editGenres;
    return editGenres.includes(current) ? editGenres : [current, ...editGenres];
  }

  function submitPlaylistEdit(): void {
    const name = editName.trim();
    if (!name) return;
    const d = overlay.dialog;
    if (d?.kind !== 'playlistEdit') return;
    const isSmart = editKind === 'smart';
    let rule = editRule;
    if (isSmart) {
      // 排序由歌单视图的列头决定；规则中的 sort/direction 仅为兼容字段，统一写入稳定的
      // title/asc。空值条件（数字清空、文本留白）整体跳过：清空数字表示删除该条件，不是填入 0
      rule = {
        ...editRule,
        sort: 'title',
        direction: 'asc',
        all: editRule.all.filter((cond) => !isEmptyCond(cond)),
      };
      const err = validateRule(rule);
      if (err) {
        notify.showError(err);
        return;
      }
    }
    const existing = d.playlist;
    const seed = d.seedIds;
    overlay.closeDialog();
    if (existing) {
      const shouldRename = existing.name !== name;
      if (isSmart) {
        // 名称与规则是两路命令：都发，规则保存自带提示
        if (shouldRename) void playlists.rename(existing, name);
        void playlists.setRule(existing, rule);
      } else if (shouldRename) {
        void playlists.rename(existing, name);
      }
    } else {
      void playlists.create(name, seed, isSmart ? rule : undefined);
    }
  }

  /* 筛选器模态框
   *
   * 形态约定：
   * ① 筛选器不复刻动态歌单的「+ 添加条件」动态行——那是规则引擎的编辑形态；
   *    筛选表达临时意图，使用固定字段表单（每字段一行，留空即不过滤）。
   * ② 控件统一使用 kit（Select / Field）：原生 select 的弹出列表不受 CSS 控制，
   *    深色主题下呈原生亮色。
   * ③ 入库时间使用日期区间，而非预设档位。
   * ④ 播放次数不作为筛选字段（常听统计已有「最近播放」入口）。
   *
   * 字段键与动态歌单相同，组装的 conds 走同一条后端解析路径，但不落库。
   */

  /** 入库时间区间草稿（YYYY-MM-DD；空串 = 不限，应用才写回，取消不落库） */
  let draftAddedFrom = $state('');
  let draftAddedTo = $state('');

  /** 各字段草稿：文本空串 / 数字 null 都表示"该字段不过滤" */
  let draftGenre = $state('');
  let draftArtist = $state('');
  let draftAlbum = $state('');
  let draftYearFrom = $state<number | null>(null);
  let draftYearTo = $state<number | null>(null);

  $effect(() => {
    const d = overlay.dialog;
    if (d?.kind !== 'trackFilter') return;
    const f = list.timeFilter;
    // 已生效条件回填到对应字段（modeOfCond 把 field+op 归回侧别；旧 UI 写出
    // 的条件均可落位，不会被静默丢弃）
    draftAddedFrom = f.addedFrom ?? '';
    draftAddedTo = f.addedTo ?? '';
    draftGenre = draftArtist = draftAlbum = '';
    draftYearFrom = draftYearTo = null;
    for (const c of f.conds) {
      const v = c.value;
      switch (modeOfCond(c)) {
        case 'genre': draftGenre = String(v); break;
        case 'artist': draftArtist = String(v); break;
        case 'album': draftAlbum = String(v); break;
        case 'yearFrom': draftYearFrom = Number(v); break;
        case 'yearTo': draftYearTo = Number(v); break;
        case 'playsMin': break; // 旧会话遗留的播放次数档：本次打开即失效
        case 'playsMax': break;
      }
    }
  });

  /** 筛选器流派下拉（与歌单编辑器同一个缓存源；独立 effect，见上面的教训注释） */
  let filterGenres = $state<string[]>([]);
  $effect(() => {
    if (overlay.dialog?.kind === 'trackFilter') {
      void library
        .genres()
        .then((g) => (filterGenres = g))
        .catch(() => (filterGenres = []));
    }
  });

  /** 流派下拉选项："任意流派"是空值语义项，放在最前 */
  const filterGenreOptions = $derived([
    { value: '', label: t('shell.filter.genreAny') },
    ...filterGenres.map((g) => ({ value: g, label: g })),
  ]);

  /** 数字输入统一归一：空串 = null（不过滤），非数字不收 */
  function numOrNull(raw: string): number | null {
    if (raw.trim() === '') return null;
    const n = Number(raw);
    return Number.isFinite(n) ? n : null;
  }

  /**
   * 应用筛选：把非空字段组装成 conds。空字段直接跳过——固定表单里"留空"
   * 就是"不过滤"，不需要报错。文本字段空值后端同样会拒绝，但这里根本
   * 不会产生空值条件。
   */
  function applyFilter(): void {
    const conds: FilterCond[] = [];
    const genre = draftGenre.trim();
    if (genre) conds.push({ field: 'genre', op: 'eq', value: genre });
    const artist = draftArtist.trim();
    if (artist) conds.push({ field: 'artist', op: 'contains', value: artist });
    const album = draftAlbum.trim();
    if (album) conds.push({ field: 'album', op: 'contains', value: album });
    if (draftYearFrom != null) conds.push({ field: 'year', op: 'gte', value: draftYearFrom });
    if (draftYearTo != null) conds.push({ field: 'year', op: 'lte', value: draftYearTo });
    overlay.closeDialog();
    list.setTimeFilter({
      addedFrom: draftAddedFrom || null,
      addedTo: draftAddedTo || null,
      conds,
    });
  }

  let deleteCancelEl = $state<HTMLButtonElement | null>(null);
  let removeCancelEl = $state<HTMLButtonElement | null>(null);

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

{#if overlay.dialog?.kind === 'alert'}
  {@const a = overlay.dialog}
  <!-- 阻断级报错弹窗：报错三档的最高档（设备丢失、未知故障）。动作键先关弹窗再执行，
       避免 run() 里再开别的浮层被本次关闭覆盖。 -->
  <Dialog title={a.title} description={a.message} onclose={() => overlay.closeDialog()}>
    {#snippet actions()}
      {#if a.action}
        <Button
          onclick={() => {
            const run = a.action!.run;
            overlay.closeDialog();
            run();
          }}
        >
          {a.action.label}
        </Button>
      {/if}
      <Button variant="primary" onclick={() => overlay.closeDialog()}>{t('common.close')}</Button>
    {/snippet}
  </Dialog>
{/if}

{#if overlay.dialog?.kind === 'playlistEdit'}
  {@const editing = overlay.dialog.playlist}
  <!-- 歌单创建与编辑共用此模态框。类型开关仅在创建时出现（后端不支持手动与智能歌单互相转换）。编辑时：手动歌单只改名称，动态歌单为名称 + 规则。条件行为「模式 select +
  动态值控件」两列；排序与上限已移除（入库按标题排序，视图内可拖拽重排，刷新只追加），编辑器仅保留条件行。名称输入框在对话框打开时初始化（见 script 中的 effect），否则歌单列表刷新会覆盖正在输入的内容。 -->
  <Dialog
    title={editing ? t('shell.playlist.editTitle') : t('shell.playlist.newTitle')}
    onclose={() => overlay.closeDialog()}
    autofocus={() => {
      nameInput?.focus();
      // 编辑场景是「改已有名字」，全选后直接输入比让用户先删更快
      if (editing) nameInput?.select();
    }}
  >
    <div class="prompt-body">
      <Field
        label={t('shell.playlist.nameLabel')}
        bind:ref={nameInput}
        value={editName}
        maxlength={80}
        oninput={(next) => (editName = next)}
        onenter={submitPlaylistEdit}
      />
      {#if !editing}
        <div class="edit-kind">
          <Switch
            checked={editKind === 'smart'}
            label={t('shell.playlist.smartToggle')}
            onchange={(v) => (editKind = v ? 'smart' : 'manual')}
          />
          <span class="edit-kind-label">{t('shell.playlist.smartToggle')}</span>
        </div>
      {/if}
      {#if editKind === 'smart'}
        <div class="rule-editor">
          <div class="rule-head">
            <Button variant="ghost" onclick={addCond}>{t('shell.playlist.addCond')}</Button>
          </div>
          {#each editRule.all as cond, i (i)}
            {@const condMode = modeOfCond(cond)}
            {@const spec = COND_MODES[condMode]}
            <div class="rule-row">
              <div class="rule-mode">
                <Select
                  label={t('shell.playlist.condLabel')}
                  value={condMode}
                  options={COND_MODE_ORDER.map((m) => ({ value: m, label: t(COND_MODE_LABEL[m]) }))}
                  onchange={(v) => setCondMode(i, v as CondMode)}
                />
              </div>
              {#if spec.control === 'genreSelect'}
                <div class="rule-value-box">
                  <Select
                    label={t('common.genre')}
                    value={String(cond.value)}
                    options={genreOptions(String(cond.value)).map((g) => ({ value: g, label: g }))}
                    placeholder={t('shell.smart.cond.genre.placeholder')}
                    onchange={(v) => (editRule.all[i] = { ...cond, value: v })}
                  />
                </div>
              {:else}
                <div class="rule-value-box">
                  <Field
                    label={t(COND_MODE_LABEL[condMode])}
                    hideLabel
                    type={spec.control === 'number' ? 'number' : 'text'}
                    placeholder={t(COND_MODE_PLACEHOLDER[condMode])}
                    value={cond.value == null ? '' : String(cond.value)}
                    oninput={(v) =>
                      (editRule.all[i] = {
                        ...cond,
                        // 数字清空 = null（未填），提交时整条跳过；不再落 0
                        value: spec.control === 'number' ? numOrNull(v) : v,
                      })}
                  />
                </div>
              {/if}
              <Button variant="ghost" ariaLabel={t('shell.playlist.removeCond')} onclick={() => removeCond(i)}>{t('shell.playlist.removeCondBtn')}</Button>
            </div>
          {:else}
            <div class="rule-empty">{t('shell.playlist.noConds')}</div>
            <p class="rule-foot">{t('shell.playlist.ruleEmptyHint')}</p>
          {/each}
          {#if editRule.all.length > 0}
            <p class="rule-foot">{t('shell.playlist.ruleHint')}</p>
          {/if}
        </div>
      {/if}
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>{t('common.cancel')}</Button>
      <Button variant="primary" disabled={!editName.trim()} onclick={submitPlaylistEdit}>
        {editing ? t('common.save') : t('common.create')}
      </Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'deletePlaylist'}
  <Dialog
    title={t('shell.playlist.deleteTitle')}
    description={t('shell.playlist.deleteDesc', { name: overlay.dialog.playlist.name })}
    onclose={() => overlay.closeDialog()}
    autofocus={() => deleteCancelEl?.focus()}
  >
    {#snippet actions()}
      <Button bind:ref={deleteCancelEl} onclick={() => overlay.closeDialog()}>{t('common.cancel')}</Button>
      <Button variant="danger" onclick={submitDelete}>{t('common.delete')}</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'removeTracks'}
  <Dialog
    title={t('shell.playlist.removeTracksTitle')}
    description={t('shell.playlist.removeTracksDesc', { n: overlay.dialog.ids.length })}
    onclose={() => overlay.closeDialog()}
    autofocus={() => removeCancelEl?.focus()}
  >
    {#snippet actions()}
      <Button bind:ref={removeCancelEl} onclick={() => overlay.closeDialog()}>{t('common.cancel')}</Button>
      <Button variant="danger" onclick={submitRemoveTracks}>{t('common.remove')}</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'trackFilter'}
  <!--
    筛选器：固定字段表单，每个可筛字段一行，留空即不过滤
    不复刻动态歌单的「+ 添加条件」动态行。入库时间为日期区间（本地时区，上界
    含当天）。字段与动态歌单同键，组装的 conds 走同一条后端解析，仅在本次查询
    生效且不落库。播放次数不是筛选字段（常听统计已有「最近播放」入口）。
    控件统一使用 kit：Select（原生下拉列表不受 CSS 控制）与 Field。
  -->
  <Dialog
    title={t('shell.filter.title')}
    description={t('shell.filter.desc')}
    onclose={() => overlay.closeDialog()}
  >
    <div class="filter-body">
      <div class="filter-row">
        <div class="filter-label">{t('shell.filter.addedTime')}</div>
        <div class="filter-pair">
          <Field
            label={t('shell.filter.addedFrom')}
            hideLabel
            type="date"
            value={draftAddedFrom}
            oninput={(v) => (draftAddedFrom = v)}
          />
          <span class="filter-dash" aria-hidden="true">–</span>
          <Field
            label={t('shell.filter.addedTo')}
            hideLabel
            type="date"
            value={draftAddedTo}
            oninput={(v) => (draftAddedTo = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">{t('common.genre')}</div>
        <div class="filter-value">
          <Select
            label={t('common.genre')}
            value={draftGenre}
            options={filterGenreOptions}
            onchange={(v) => (draftGenre = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">{t('common.artist')}</div>
        <div class="filter-value">
          <Field
            label={t('shell.filter.artistLabel')}
            hideLabel
            type="text"
            placeholder={t('shell.filter.artistPlaceholder')}
            value={draftArtist}
            oninput={(v) => (draftArtist = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">{t('common.album')}</div>
        <div class="filter-value">
          <Field
            label={t('shell.filter.albumLabel')}
            hideLabel
            type="text"
            placeholder={t('shell.filter.albumPlaceholder')}
            value={draftAlbum}
            oninput={(v) => (draftAlbum = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">{t('common.year')}</div>
        <div class="filter-pair">
          <Field
            label={t('shell.filter.yearFrom')}
            hideLabel
            type="number"
            placeholder={t('shell.filter.yearFromShort')}
            value={String(draftYearFrom ?? '')}
            oninput={(v) => (draftYearFrom = numOrNull(v))}
          />
          <span class="filter-dash" aria-hidden="true">–</span>
          <Field
            label={t('shell.filter.yearTo')}
            hideLabel
            type="number"
            placeholder={t('shell.filter.yearToShort')}
            value={String(draftYearTo ?? '')}
            oninput={(v) => (draftYearTo = numOrNull(v))}
          />
        </div>
      </div>
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>{t('common.cancel')}</Button>
      <Button variant="primary" onclick={applyFilter}>{t('common.apply')}</Button>
    {/snippet}
  </Dialog>
{:else if overlay.dialog?.kind === 'addToPlaylist'}
  <!-- 极简歌单选择器：无大标题、无关闭键、无底部按钮。顶部仅一行次要文本说明用途，主体为歌单列表，点击行即添加并关闭；无歌单时显示一行空态文案。关闭走 Esc 或点击遮罩。不使用
  Menu：选择歌单属于需要看到各歌单曲目数的浏览行为，而菜单面向 "已知要点哪一项"的快捷路径，两者交互模型不同。 -->
  <Dialog
    label={t('shell.pick.title')}
    description={t('shell.pick.title')}
    width="md"
    onclose={() => overlay.closeDialog()}
  >
    <div class="pick-list">
      <!-- 动态歌单与手动歌单能力一致（可手动加歌），不按 kind 过滤 -->
      {#each playlists.items as playlist (playlist.id)}
        <button class="pick-row" onclick={() => submitAddTo(playlist.id)}>
          <Cover src={playlist.artworkId ? artworkUrl(playlist.artworkId, 96) : null} size="sm" />
          <span class="pick-name">{playlist.name}</span>
          <span class="pick-count">{t('common.trackCount', { n: playlist.trackCount })}</span>
        </button>
      {:else}
        <p class="pick-empty">{t('shell.pick.empty')}</p>
      {/each}
    </div>
  </Dialog>
{/if}

<style>
  /* 输入对话框与歌单选择器的主体样式（随内联模板一并迁入，原为独立组件的 scoped 样式） */
  .prompt-body {
    padding: 14px 16px 2px;
  }

  /* 动态歌单开关行：开关 + 可见标题（Switch 的 label 只供读屏，可见文本在这里） */
  .edit-kind {
    margin-top: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .edit-kind-label {
    font-size: var(--fs-base);
    color: var(--text-1);
  }

  /* 规则编辑区：上分隔线与名称/开关区分开；添加条件按钮靠左 */
  .rule-editor {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .rule-head {
    display: flex;
    align-items: center;
  }

  .rule-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* 条件行：模式下拉定宽、值控件占满剩余（控件本体是 kit 的 Select/Field） */
  .rule-mode {
    flex: none;
    width: 118px;
  }

  .rule-value-box {
    flex: 1;
    min-width: 0;
  }

  /* 无条件占位块 + 下方一句说明（复用 .rule-foot 的样式，与有条件态的
     收尾说明同一观感）：语义说明放在块外，块本身只留占位文本 */
  .rule-empty {
    display: grid;
    place-items: center;
    padding: 14px 0;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-md);
    font-size: var(--fs-base);
    color: var(--text-3);
  }

  .rule-foot {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-3);
  }



  /* 筛选器模态框：固定字段表单——标签列定宽，值控件占满剩余空间 */
  .filter-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 16px 4px;
  }

  .filter-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .filter-label {
    flex: none;
    width: 56px;
    font-size: var(--fs-base);
    color: var(--text-2);
  }

  .filter-value {
    flex: 1;
    min-width: 0;
  }

  /* 日期区间 / 年份区间的"从–到"一对控件 */
  .filter-pair {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* Field 的根节点是子组件内部元素，scoped 选择器够不到，:global 兜住 */
  .filter-pair > :global(.k-field) {
    flex: 1;
    min-width: 0;
  }

  .filter-dash {
    flex: none;
    color: var(--text-3);
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
    font-size: var(--fs-base);
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
    font-size: var(--fs-sm);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }

  .pick-empty {
    margin: 0;
    padding: 18px 10px;
    font-size: var(--fs-base);
    color: var(--text-2);
    text-align: center;
  }
</style>
