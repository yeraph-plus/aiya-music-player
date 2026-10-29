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
      // 详情页上"查看自己"没有意义（docs/28，与列链接同一条规则）
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

  /* ---------------- 歌单统一模态框（创建/编辑同一个，2026-10-28 拍板） ----------------
   *
   * 以前是 createPlaylist / renamePlaylist 两个对话框，只差一个标题和初始值；
   * 动态歌单进来后"编辑"要带上规则编辑器，两个壳必然合一个。规则草稿只在
   * 打开那一刻从 playlist 快照，提交才写回（取消不落库），与筛选器同款。
   */

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

  function addCond(): void {
    // 新条件默认流派——这是用户最常用的入口（docs/40 §1.2 的标签歌单）
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

  /** 流派下拉选项：当前值不在曲库流派里时插到最前（旧规则值不静默丢失） */
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
      // 排序归歌单视图的列头（2026-10-28 拍板），规则里的 sort/direction
      // 只是兼容字段，统一写稳定的 title/asc。空值条件（数字清空/文本留白）
      // 整体跳过——清空数字是"删掉这个条件"，不是"填 0"
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

  /* ---------------- 筛选器模态框（docs/27 第 2/3 点 + 2026-10-28 多轮修订） ----------------
   *
   * 五轮拍板：① 筛选器**不复刻**动态歌单的「+ 添加条件」动态行——那是
   * 规则引擎的编辑形态，筛选是临时意图，用固定字段表单（每字段一行，
   * 留空 = 不过滤）。② 控件全部走 kit（Select/Field），原生 select 的
   * 弹出列表不受 CSS 管，深色主题里是刺眼的原生亮色条。③ 入库时间是
   * **日期区间**而非预设档。④ 播放次数不是筛选字段（常听统计已有
   * 「最近播放」入口）；字段与动态歌单同键，组装的 conds 走同一条后端
   * 解析，只是不落库。
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
    // 的条件都能落位，不会静默丢档）
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
    { value: '', label: '任意流派' },
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

{#if overlay.dialog?.kind === 'playlistEdit'}
  {@const editing = overlay.dialog.playlist}
  <!--
    歌单创建/编辑同一个模态框（2026-10-28 拍板，同日二轮修订）。
    类型开关只在创建时出现（后端不支持手动↔智能转换）；手动歌单编辑 =
    只改名称，动态歌单编辑 = 名称 + 规则。条件行是「模式 select + 动态值
    控件」两列；排序/上限都已退役（2026-10-30 物化：入库按标题排、视图里
    可拖拽重排，刷新只追加），编辑器只剩条件行。输入框 text 在
    对话框打开时初始化（见 script 里的 effect），否则歌单列表刷新会覆盖
    用户正在输的字。
  -->
  <Dialog
    title={editing ? '编辑播放列表' : '新建播放列表'}
    onclose={() => overlay.closeDialog()}
    autofocus={() => {
      nameInput?.focus();
      // 编辑场景是「改已有名字」，全选后直接输入比让用户先删更快
      if (editing) nameInput?.select();
    }}
  >
    <div class="prompt-body">
      <Field
        label="名称"
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
            label="动态歌单"
            onchange={(v) => (editKind = v ? 'smart' : 'manual')}
          />
          <span class="edit-kind-label">动态歌单</span>
          <span class="dim">按条件收录曲目，「刷新」追加新命中，其余与普通歌单一致</span>
        </div>
      {/if}
      {#if editKind === 'smart'}
        <div class="rule-editor">
          <div class="rule-head">
            <span class="rule-title">满足以下全部条件</span>
            <button class="link" type="button" onclick={addCond}>+ 添加条件</button>
          </div>
          {#each editRule.all as cond, i (i)}
            {@const spec = COND_MODES[modeOfCond(cond)]}
            <div class="rule-row">
              <div class="rule-mode">
                <Select
                  label="条件"
                  value={modeOfCond(cond)}
                  options={COND_MODE_ORDER.map((m) => ({ value: m, label: COND_MODES[m].label }))}
                  onchange={(v) => setCondMode(i, v as CondMode)}
                />
              </div>
              {#if spec.control === 'genreSelect'}
                <div class="rule-value-box">
                  <Select
                    label="流派"
                    value={String(cond.value)}
                    options={genreOptions(String(cond.value)).map((g) => ({ value: g, label: g }))}
                    placeholder="选择流派"
                    onchange={(v) => (editRule.all[i] = { ...cond, value: v })}
                  />
                </div>
              {:else}
                <div class="rule-value-box">
                  <Field
                    label={spec.label}
                    hideLabel
                    type={spec.control === 'number' ? 'number' : 'text'}
                    placeholder={spec.placeholder}
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
              <button class="link rule-del" type="button" aria-label="删除条件" onclick={() => removeCond(i)}>×</button>
            </div>
          {:else}
            <p class="rule-empty dim">无条件 = 匹配整个曲库</p>
          {/each}
          <div class="rule-row rule-meta">
            <span class="rule-title">排序</span>
            <span class="dim">入库顺序按标题排；在歌单里可拖拽重排，刷新只追加新命中的曲目</span>
          </div>
        </div>
      {/if}
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="primary" disabled={!editName.trim()} onclick={submitPlaylistEdit}>
        {editing ? '保存' : '创建'}
      </Button>
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
    筛选器（docs/27 第 2/3 点 + 2026-10-28 五轮拍板）：**固定字段表单**，
    每个可筛字段一行、留空即不过滤——不复刻动态歌单的「+ 添加条件」动态
    行。入库时间是日期区间（本地时区，上界含当天）；字段与动态歌单同键，
    组装的 conds 走同一条后端解析、只在本次查询生效、不落库；播放次数
    不是筛选字段（常听统计已有「最近播放」入口）。控件全走 kit：
    Select（原生下拉列表不受 CSS 管）/ Field。
  -->
  <Dialog
    title="筛选曲目"
    description="按入库时间或流派/艺人/专辑/年份过滤当前列表；留空的字段不过滤，换页即清"
    onclose={() => overlay.closeDialog()}
  >
    <div class="filter-body">
      <div class="filter-row">
        <div class="filter-label">入库时间</div>
        <div class="filter-pair">
          <Field
            label="入库时间从"
            hideLabel
            type="date"
            value={draftAddedFrom}
            oninput={(v) => (draftAddedFrom = v)}
          />
          <span class="filter-dash" aria-hidden="true">–</span>
          <Field
            label="入库时间到"
            hideLabel
            type="date"
            value={draftAddedTo}
            oninput={(v) => (draftAddedTo = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">流派</div>
        <div class="filter-value">
          <Select
            label="流派"
            value={draftGenre}
            options={filterGenreOptions}
            onchange={(v) => (draftGenre = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">艺人</div>
        <div class="filter-value">
          <Field
            label="艺人关键词"
            hideLabel
            type="text"
            placeholder="艺人名关键词"
            value={draftArtist}
            oninput={(v) => (draftArtist = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">专辑</div>
        <div class="filter-value">
          <Field
            label="专辑关键词"
            hideLabel
            type="text"
            placeholder="专辑名关键词"
            value={draftAlbum}
            oninput={(v) => (draftAlbum = v)}
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-label">年份</div>
        <div class="filter-pair">
          <Field
            label="年份不早于"
            hideLabel
            type="number"
            placeholder="不早于"
            value={String(draftYearFrom ?? '')}
            oninput={(v) => (draftYearFrom = numOrNull(v))}
          />
          <span class="filter-dash" aria-hidden="true">–</span>
          <Field
            label="年份不晚于"
            hideLabel
            type="number"
            placeholder="不晚于"
            value={String(draftYearTo ?? '')}
            oninput={(v) => (draftYearTo = numOrNull(v))}
          />
        </div>
      </div>
    </div>
    {#snippet actions()}
      <Button onclick={() => overlay.closeDialog()}>取消</Button>
      <Button variant="primary" onclick={applyFilter}>应用</Button>
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
      <!-- 物化模型（2026-10-30）后动态歌单与手动歌单能力一致，可手动加歌，
           不再按 kind 过滤 -->
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

  /* 动态歌单开关行 */
  .edit-kind {
    margin-top: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .edit-kind-label {
    font-size: 12.5px;
    color: var(--text-1);
  }

  .rule-editor {
    margin-top: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .rule-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .rule-title {
    font-size: 12px;
    color: var(--text-2);
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

  .rule-del {
    flex: none;
    font-size: 14px;
    line-height: 1;
    padding: 2px 6px;
  }

  .rule-empty {
    margin: 0;
    font-size: 12px;
  }

  .rule-meta {
    margin-top: 4px;
    gap: 12px;
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
    font-size: 12.5px;
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
