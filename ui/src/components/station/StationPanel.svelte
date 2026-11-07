<script lang="ts">
  /**
   * 推流频道管理器：LiveView「我的频道」tab 的内容。三个视图：频道列表 →
   * 详情（基本信息 + 歌单编排 + 周×24h 节目表网格）→ 新建/编辑表单。歌单与
   * 节目表都是整表提交（server 事务全删全插）。所有操作后 `onchanged()` 回
   * 调——父层负责把站单合并进收听列表。仅在 caps.streaming 形态渲染（桌面
   * 壳没有这些命令）。
   */
  import { t } from '../../lib/i18n.svelte';
  import { ipc } from '../../lib/ipc/client';
  import type { StationDetail, StationSchedule, StationSummary } from '../../lib/ipc/generated';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import { notify } from '../../stores/notify.svelte';
  import {
    Plus, Pencil, Trash2, Play, Square, SkipForward, ArrowLeft, Copy, Check,
  } from '@lucide/svelte';

  /** 站单已变（开播/停播/元数据保存）——父层拉 station_list 合并收听列表 */
  let { onchanged = () => {} }: { onchanged?: () => void } = $props();

  let stations = $state<StationSummary[]>([]);
  let selectedId = $state<string | null>(null);
  let detail = $state<StationDetail | null>(null);
  let mode = $state<'list' | 'detail' | 'edit'>('list');
  let busy = $state(false);
  let copied = $state(false);

  /* ---- 编辑表单 ---- */
  let form = $state({ name: '', subtitle: '', description: '', coverUrl: '', visibility: 'private' });
  let editingId = $state<string | null>(null);

  /* ---- 歌单对话框 ---- */
  let plDlg = $state(false);
  let plDraft = $state<{ name: string; kind: 'manual' | 'random_all'; trackIds: string[] }>({
    name: '',
    kind: 'random_all',
    trackIds: [],
  });
  let plEditIndex = $state<number | null>(null);
  let plSearch = $state('');
  let plSearchHits = $state<{ id: string; title: string; artist: string | null }[]>([]);
  let plSearchPage = $state(0);

  /* ---- 时段对话框 ---- */
  let schDlg = $state(false);
  let schDraft = $state({ playlistId: '', startH: 0, endH: 1, days: [] as number[], priority: 0 });
  let schEditIndex = $state<number | null>(null);

  const DAYS = ['station.schMon', 'station.schTue', 'station.schWed', 'station.schThu', 'station.schFri', 'station.schSat', 'station.schSun'] as const;
  const HOURS = Array.from({ length: 24 }, (_, i) => i);

  export function refresh(): void {
    void load();
  }

  // 挂载即拉：tab 切换会卸载/重挂本组件，列表态必须自恢复
  $effect(() => {
    void load();
  });

  async function load(): Promise<void> {
    stations = await ipc.station.list();
    if (selectedId) {
      const still = stations.find((s) => s.id === selectedId);
      if (still) detail = await ipc.station.get(selectedId);
      else selectedId = null;
    }
  }

  /* ---- 列表操作 ---- */
  function openDetail(id: string): void {
    selectedId = id;
    mode = 'detail';
    void load();
  }

  function openNew(): void {
    editingId = null;
    form = { name: '', subtitle: '', description: '', coverUrl: '', visibility: 'private' };
    mode = 'edit';
  }

  function openEdit(): void {
    if (!detail) return;
    editingId = detail.id;
    form = {
      name: detail.name,
      subtitle: detail.subtitle ?? '',
      description: detail.description ?? '',
      coverUrl: detail.coverUrl ?? '',
      visibility: detail.visibility,
    };
    mode = 'edit';
  }

  async function saveForm(): Promise<void> {
    if (!form.name.trim() || busy) return;
    busy = true;
    try {
      const input = {
        name: form.name.trim(),
        subtitle: form.subtitle.trim() || null,
        description: form.description.trim() || null,
        coverUrl: form.coverUrl.trim() || null,
        visibility: form.visibility,
      };
      const saved = editingId
        ? await ipc.station.update(editingId, input)
        : await ipc.station.create(input);
      selectedId = saved.id;
      mode = 'detail';
      notify.showToast(t('station.toastSaved'));
      await load();
      onchanged();
    } finally {
      busy = false;
    }
  }

  async function toggleActive(s: StationSummary): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await ipc.station.activate(s.id, !s.active);
      notify.showToast(t(s.active ? 'station.toastOff' : 'station.toastOn', { name: s.name }));
      await load();
      onchanged();
    } finally {
      busy = false;
    }
  }

  async function doSkip(s: StationSummary): Promise<void> {
    await ipc.station.skip(s.id);
    notify.showToast(t('station.toastSkip'));
    await load();
  }

  async function doDelete(s: StationSummary): Promise<void> {
    if (!confirm(t('station.delConfirm', { name: s.name }))) return;
    await ipc.station.remove(s.id);
    notify.showToast(t('station.toastDeleted'));
    if (selectedId === s.id) {
      selectedId = null;
      mode = 'list';
    }
    await load();
    onchanged();
  }

  async function copyHls(s: StationSummary): Promise<void> {
    await navigator.clipboard.writeText(new URL(s.hlsUrl, location.origin).toString());
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  /* ---- 歌单 ---- */
  function openPlNew(): void {
    plEditIndex = null;
    plDraft = { name: '', kind: 'random_all', trackIds: [] };
    plSearch = '';
    plSearchHits = [];
    plSearchPage = 0;
    plDlg = true;
  }

  function openPlEdit(i: number): void {
    const p = detail!.playlists[i];
    plEditIndex = i;
    plDraft = { name: p.name, kind: p.kind as 'manual' | 'random_all', trackIds: [...p.trackIds] };
    plSearch = '';
    plSearchHits = [];
    plSearchPage = 0;
    plDlg = true;
  }

  async function searchTracks(): Promise<void> {
    const q = plSearch.trim();
    if (!q) return;
    const page = await ipc.library.list({ search: q, offset: plSearchPage * 30, limit: 30 });
    plSearchHits = page.items.map((tr) => ({ id: tr.id, title: tr.title, artist: tr.artist }));
  }

  function toggleHit(id: string): void {
    plDraft.trackIds = plDraft.trackIds.includes(id)
      ? plDraft.trackIds.filter((x) => x !== id)
      : [...plDraft.trackIds, id];
  }

  async function savePlaylist(): Promise<void> {
    if (!detail || !plDraft.name.trim() || busy) return;
    busy = true;
    try {
      const list = detail.playlists.map((p) => ({
        name: p.name,
        kind: p.kind,
        trackIds: p.trackIds.length > 0 ? p.trackIds : null,
        ruleJson: p.ruleJson,
      }));
      const payload = {
        name: plDraft.name.trim(),
        kind: plDraft.kind,
        trackIds: plDraft.kind === 'manual' ? plDraft.trackIds : null,
        ruleJson: null,
      };
      if (plEditIndex === null) list.push(payload);
      else list[plEditIndex] = payload;
      const { playlists } = await ipc.station.setPlaylists(detail.id, list);
      detail = { ...detail!, playlists };
      plDlg = false;
      notify.showToast(t('station.toastRestart'));
      onchanged();
    } finally {
      busy = false;
    }
  }

  async function removePlaylist(i: number): Promise<void> {
    if (!detail || busy) return;
    busy = true;
    try {
      const list = detail.playlists.filter((_, x) => x !== i);
      const { playlists } = await ipc.station.setPlaylists(detail.id, list);
      detail = { ...detail!, playlists };
      onchanged();
    } finally {
      busy = false;
    }
  }

  /* ---- 节目表网格 ---- */
  const PL_COLORS = ['var(--accent)', '#7aa2f7', '#9ece6a', '#e0af68', '#bb9af7', '#f7768e'];

  function plColor(playlistId: string): string {
    const i = detail?.playlists.findIndex((p) => p.id === playlistId) ?? -1;
    return PL_COLORS[Math.max(0, i) % PL_COLORS.length];
  }

  function plName(playlistId: string): string {
    return detail?.playlists.find((p) => p.id === playlistId)?.name ?? '—';
  }

  /** schedules 展开：day(0-6) × hour(0-23) → 最高优先级的 schedule index */
  function gridAt(day: number, hour: number): number | null {
    if (!detail) return null;
    let best = -1;
    let hit: number | null = null;
    detail.schedules.forEach((s: StationSchedule, i: number) => {
      if ((s.daysMask & (1 << day)) === 0) return;
      const sh = parseInt(s.startHm, 10);
      const eh = parseInt(s.endHm, 10);
      const covers = sh < eh ? hour >= sh && hour < eh : hour >= sh || hour < eh;
      if (covers && s.priority >= best) {
        best = s.priority;
        hit = i;
      }
    });
    return hit;
  }

  function hourLabel(h: number): string {
    return `${String(h).padStart(2, '0')}`;
  }

  function openSchNew(day: number, hour: number): void {
    if (!detail) return;
    schEditIndex = null;
    schDraft = {
      playlistId: detail.playlists[0]?.id ?? '',
      startH: hour,
      endH: (hour + 1) % 24,
      days: [day],
      priority: 0,
    };
    schDlg = true;
  }

  function openSchEdit(i: number): void {
    const s = detail!.schedules[i];
    schEditIndex = i;
    schDraft = {
      playlistId: s.playlistId,
      startH: parseInt(s.startHm, 10),
      endH: parseInt(s.endHm, 10),
      days: [0, 1, 2, 3, 4, 5, 6].filter((d) => (s.daysMask & (1 << d)) !== 0),
      priority: s.priority,
    };
    schDlg = true;
  }

  async function saveSchedule(): Promise<void> {
    if (!detail || !schDraft.playlistId || busy) return;
    busy = true;
    try {
      const hm = (h: number) => `${String(h).padStart(2, '0')}:00`;
      const list = detail.schedules.map((sc) => ({
        playlistId: sc.playlistId,
        startHm: sc.startHm,
        endHm: sc.endHm,
        daysMask: sc.daysMask,
        priority: sc.priority,
      }));
      const payload = {
        playlistId: schDraft.playlistId,
        startHm: hm(schDraft.startH),
        endHm: hm(schDraft.endH),
        daysMask: schDraft.days.reduce((acc, d) => acc | (1 << d), 0),
        priority: schDraft.priority,
      };
      if (schEditIndex === null) list.push(payload);
      else list[schEditIndex] = payload;
      const { schedules } = await ipc.station.setSchedules(detail.id, list);
      detail = { ...detail!, schedules };
      schDlg = false;
      notify.showToast(t('station.toastRestart'));
    } finally {
      busy = false;
    }
  }

  async function removeSchedule(): Promise<void> {
    if (!detail || schEditIndex === null || busy) return;
    busy = true;
    try {
      const list = detail.schedules.map((sc) => ({
        playlistId: sc.playlistId,
        startHm: sc.startHm,
        endHm: sc.endHm,
        daysMask: sc.daysMask,
        priority: sc.priority,
      }));
      list.splice(schEditIndex, 1);
      const { schedules } = await ipc.station.setSchedules(detail.id, list);
      detail = { ...detail!, schedules };
      schDlg = false;
      notify.showToast(t('station.toastRestart'));
    } finally {
      busy = false;
    }
  }
</script>

<div class="station">
  {#if mode === 'list'}
    <div class="st-list">
      <div class="st-list-head">
        <div>
          <h3 class="st-title">{t('station.listTitle')}</h3>
          <p class="st-sub">{t('station.listSubtitle')}</p>
        </div>
        <Button variant="primary" onclick={openNew}>
          {#snippet icon()}<Plus size={14} />{/snippet}
          {t('station.new')}
        </Button>
      </div>
      {#if stations.length === 0}
        <p class="st-empty">{t('station.empty')}</p>
      {:else}
        <ul class="st-cards">
          {#each stations as s (s.id)}
            <li class="st-card">
              <span class="st-dot" class:on={s.running} title={s.running ? t('station.stateOn') : t('station.stateOff')}></span>
              <div class="st-card-main">
                <button class="st-card-name" onclick={() => openDetail(s.id)}>{s.name}</button>
                <span class="st-card-meta">
                  {s.running ? t('station.stateOn') : t('station.stateOff')}
                  {#if s.nowPlaying}
                    · {t('station.nowPlaying', { title: s.nowPlaying })}
                  {/if}
                  · {s.visibility === 'public' ? t('station.visPublic') : t('station.visPrivate')}
                </span>
              </div>
              <div class="st-card-ops">
                <IconButton label={t('station.copyHls')} size="sm" onclick={() => void copyHls(s)}>{#if copied}<Check size={13} />{:else}<Copy size={13} />{/if}</IconButton>
                <IconButton label={t('station.skip')} size="sm" disabled={!s.running} onclick={() => void doSkip(s)}><SkipForward size={13} /></IconButton>
                <Button variant={s.active ? 'ghost' : 'primary'} disabled={busy} onclick={() => void toggleActive(s)}>
                  {#snippet icon()}{#if s.active}<Square size={13} />{:else}<Play size={13} />{/if}{/snippet}
                  {s.active ? t('station.stop') : t('station.play')}
                </Button>
                <IconButton label={t('station.edit')} size="sm" onclick={() => openDetail(s.id)}><Pencil size={13} /></IconButton>
                <IconButton label={t('station.del')} size="sm" onclick={() => void doDelete(s)}><Trash2 size={13} /></IconButton>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else if mode === 'edit'}
    <div class="st-form">
      <button class="st-back" onclick={() => (mode = selectedId ? 'detail' : 'list')}>
        <ArrowLeft size={14} /> {t('station.back')}
      </button>
      <h3 class="st-title">{editingId ? t('station.formTitleEdit') : t('station.formTitleNew')}</h3>
      <label class="st-field">
        <span>{t('station.fieldName')}</span>
        <input bind:value={form.name} placeholder={t('station.fieldNamePh')} />
      </label>
      <label class="st-field">
        <span>{t('station.fieldSubtitle')}</span>
        <input bind:value={form.subtitle} placeholder={t('station.fieldSubtitlePh')} />
      </label>
      <label class="st-field">
        <span>{t('station.fieldDesc')}</span>
        <textarea bind:value={form.description} rows="2" placeholder={t('station.fieldDescPh')}></textarea>
      </label>
      <label class="st-field">
        <span>{t('station.fieldCover')}</span>
        <input bind:value={form.coverUrl} placeholder={t('station.fieldCoverPh')} />
      </label>
      <label class="st-field">
        <span>{t('station.fieldVisibility')}</span>
        <select bind:value={form.visibility}>
          <option value="private">{t('station.visPrivate')}</option>
          <option value="public">{t('station.visPublic')}</option>
        </select>
      </label>
      {#if form.visibility === 'public'}
        <p class="st-hint">{t('station.visHint')}</p>
      {/if}
      <div class="st-form-ops">
        <Button variant="primary" disabled={busy || !form.name.trim()} onclick={() => void saveForm()}>
          {t('station.save')}
        </Button>
        <Button variant="ghost" onclick={() => (mode = selectedId ? 'detail' : 'list')}>
          {t('station.cancel')}
        </Button>
      </div>
    </div>
  {:else if mode === 'detail' && detail}
    <div class="st-detail">
      <button class="st-back" onclick={() => (mode = 'list')}>
        <ArrowLeft size={14} /> {t('station.back')}
      </button>
      <div class="st-detail-head">
        <h3 class="st-title">{detail.name}</h3>
        <span class="st-dot" class:on={detail.running}></span>
        <span class="st-card-meta">{detail.running ? t('station.stateOn') : t('station.stateOff')}</span>
        <div class="st-detail-ops">
          <Button variant="ghost" onclick={openEdit}>
            {#snippet icon()}<Pencil size={13} />{/snippet}
            {t('station.edit')}
          </Button>
          <Button variant={detail.active ? 'ghost' : 'primary'} disabled={busy} onclick={() => void toggleActive(detail!)}>
            {#snippet icon()}{#if detail!.active}<Square size={13} />{:else}<Play size={13} />{/if}{/snippet}
            {detail.active ? t('station.stop') : t('station.play')}
          </Button>
          {#if detail.running}
            <Button variant="ghost" onclick={() => void doSkip(detail!)}>
              {#snippet icon()}<SkipForward size={13} />{/snippet}
              {t('station.skip')}
            </Button>
          {/if}
        </div>
      </div>

      <!-- 歌单 -->
      <div class="st-sec">
        <div class="st-sec-head">
          <h4>{t('station.plTitle')}</h4>
          <Button variant="ghost" onclick={openPlNew}>
            {#snippet icon()}<Plus size={13} />{/snippet}
            {t('station.plNew')}
          </Button>
        </div>
        {#if detail.playlists.length === 0}
          <p class="st-empty">{t('station.plEmpty')}</p>
        {:else}
          <ul class="st-pls">
            {#each detail.playlists as p, i (p.id)}
              <li class="st-pl">
                <span class="st-pl-swatch" style:background={plColor(p.id)}></span>
                <span class="st-pl-name">{p.name}</span>
                <span class="st-card-meta">{t(p.kind === 'manual' ? 'station.plKindManual' : 'station.plKindRandomAll')}</span>
                <span class="st-card-meta">{t('station.plTracks', { count: p.trackIds.length })}</span>
                <span class="st-pl-ops">
                  <IconButton label={t('station.edit')} size="sm" onclick={() => openPlEdit(i)}><Pencil size={12} /></IconButton>
                  <IconButton label={t('station.del')} size="sm" onclick={() => void removePlaylist(i)}><Trash2 size={12} /></IconButton>
                </span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- 节目表网格 -->
      <div class="st-sec">
        <div class="st-sec-head">
          <h4>{t('station.schTitle')}</h4>
        </div>
        <p class="st-hint">{t('station.schSubtitle')}</p>
        {#if detail.playlists.length > 0 && detail.schedules.length > 0}
          <p class="st-hint">{t('station.schDefault', { name: plName(detail.schedules[0].playlistId) })}</p>
        {/if}
        <div class="st-grid" role="grid" aria-label={t('station.schTitle')}>
          <div class="st-grid-corner"></div>
          {#each DAYS as dayKey (dayKey)}
            <span class="st-grid-day">{t(dayKey)}</span>
          {/each}
          {#each HOURS as h (h)}
            <span class="st-grid-hour">{hourLabel(h)}</span>
            {#each DAYS as _, day (day)}
              {@const hit = gridAt(day, h)}
              <button
                class="st-cell"
                class:filled={hit !== null}
                style:background={hit !== null ? plColor(detail!.schedules[hit].playlistId) : undefined}
                title={hit !== null ? `${t('station.edit')} · ${plName(detail!.schedules[hit].playlistId)}` : `${hourLabel(h)}:00`}
                onclick={() => (hit !== null ? openSchEdit(hit) : openSchNew(day, h))}
              ></button>
            {/each}
          {/each}
        </div>
      </div>
    </div>
  {/if}

  <!-- 歌单对话框 -->
  {#if plDlg}
    <div class="st-dlg" role="dialog" aria-modal="true">
      <div class="st-dlg-body">
        <h4>{plEditIndex === null ? t('station.plNew') : t('station.edit')}</h4>
        <label class="st-field">
          <span>{t('station.plNamePh')}</span>
          <input bind:value={plDraft.name} placeholder={t('station.plNamePh')} />
        </label>
        <label class="st-field">
          <span>{t('station.schPlaylist')}</span>
          <select bind:value={plDraft.kind}>
            <option value="random_all">{t('station.plKindRandomAll')}</option>
            <option value="manual">{t('station.plKindManual')}</option>
          </select>
        </label>
        <p class="st-hint">{plDraft.kind === 'manual' ? t('station.plManualHint') : t('station.plRandomHint')}</p>
        {#if plDraft.kind === 'manual'}
          <label class="st-field">
            <span>{t('station.plSearch')}</span>
            <input
              bind:value={plSearch}
              placeholder={t('station.plSearch')}
              onkeydown={(e) => e.key === 'Enter' && void searchTracks()}
            />
          </label>
          <Button variant="ghost" onclick={() => void searchTracks()}>{t('station.plSearch')}</Button>
          <ul class="st-hits">
            {#each plSearchHits as h (h.id)}
              <li>
                <label class="st-hit">
                  <input type="checkbox" checked={plDraft.trackIds.includes(h.id)} onchange={() => toggleHit(h.id)} />
                  <span>{h.title}{h.artist ? ` — ${h.artist}` : ''}</span>
                </label>
              </li>
            {:else}
              {#if plSearch.trim()}
                <li class="st-empty">{t('station.plSearchEmpty')}</li>
              {/if}
            {/each}
          </ul>
          <p class="st-card-meta">{t('station.plTracks', { count: plDraft.trackIds.length })}</p>
        {/if}
        <div class="st-form-ops">
          <Button variant="primary" disabled={busy || !plDraft.name.trim()} onclick={() => void savePlaylist()}>
            {t('station.save')}
          </Button>
          <Button variant="ghost" onclick={() => (plDlg = false)}>{t('station.cancel')}</Button>
        </div>
      </div>
    </div>
  {/if}

  <!-- 时段对话框 -->
  {#if schDlg}
    <div class="st-dlg" role="dialog" aria-modal="true">
      <div class="st-dlg-body">
        <h4>{t('station.schEditTitle')}</h4>
        <div class="st-row2">
          <label class="st-field">
            <span>{t('station.schStart')}</span>
            <select bind:value={schDraft.startH}>
              {#each HOURS as h (h)}<option value={h}>{hourLabel(h)}:00</option>{/each}
            </select>
          </label>
          <label class="st-field">
            <span>{t('station.schEnd')}</span>
            <select bind:value={schDraft.endH}>
              {#each HOURS as h (h)}<option value={h}>{hourLabel(h)}:00</option>{/each}
            </select>
          </label>
        </div>
        {#if schDraft.endH <= schDraft.startH}
          <p class="st-hint">{t('station.schCrossMidnight')} — {t('station.schEndHint')}</p>
        {/if}
        <span class="st-field-label">{t('station.schDays')}</span>
        <div class="st-days">
          {#each DAYS as dayKey, day (dayKey)}
            <button
              class="st-day"
              class:on={schDraft.days.includes(day)}
              onclick={() =>
                (schDraft.days = schDraft.days.includes(day)
                  ? schDraft.days.filter((x) => x !== day)
                  : [...schDraft.days, day])}
            >
              {t(dayKey)}
            </button>
          {/each}
        </div>
        <label class="st-field">
          <span>{t('station.schPlaylist')}</span>
          <select bind:value={schDraft.playlistId}>
            {#each detail?.playlists ?? [] as p (p.id)}
              <option value={p.id}>{p.name} · {t(p.kind === 'manual' ? 'station.plKindManual' : 'station.plKindRandomAll')}</option>
            {/each}
          </select>
        </label>
        <label class="st-field">
          <span>{t('station.schPriority')}</span>
          <input type="number" bind:value={schDraft.priority} />
        </label>
        <p class="st-hint">{t('station.schPriorityHint')}</p>
        <div class="st-form-ops">
          {#if schEditIndex !== null}
            <Button variant="ghost" onclick={() => void removeSchedule()}>{t('station.schDelete')}</Button>
          {/if}
          <Button variant="primary" disabled={busy || !schDraft.playlistId || schDraft.days.length === 0} onclick={() => void saveSchedule()}>
            {t('station.save')}
          </Button>
          <Button variant="ghost" onclick={() => (schDlg = false)}>{t('station.cancel')}</Button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .station { display: flex; flex-direction: column; gap: 16px; }
  .st-title { margin: 0; font-size: 18px; }
  .st-sub { margin: 4px 0 0; opacity: 0.65; font-size: 13px; }
  .st-empty { opacity: 0.6; font-size: 13px; }
  .st-hint { opacity: 0.6; font-size: 12px; margin: 4px 0; }
  .st-list-head, .st-sec-head, .st-detail-head { display: flex; align-items: center; gap: 12px; justify-content: space-between; }
  .st-detail-head { justify-content: flex-start; flex-wrap: wrap; }
  .st-cards { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .st-card { display: flex; align-items: center; gap: 12px; padding: 12px 14px; border: 1px solid color-mix(in srgb, currentColor 12%, transparent); border-radius: 12px; }
  .st-dot { width: 8px; height: 8px; border-radius: 50%; background: color-mix(in srgb, currentColor 30%, transparent); flex: none; }
  .st-dot.on { background: #9ece6a; box-shadow: 0 0 8px #9ece6a88; }
  .st-card-main { display: flex; flex-direction: column; gap: 2px; flex: 1; min-width: 0; }
  .st-card-name { background: none; border: none; color: inherit; font: inherit; font-weight: 600; text-align: left; padding: 0; cursor: pointer; }
  .st-card-meta { font-size: 12px; opacity: 0.65; }
  .st-card-ops, .st-pl-ops, .st-detail-ops { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .st-back { display: inline-flex; align-items: center; gap: 6px; background: none; border: none; color: inherit; cursor: pointer; font: inherit; opacity: 0.75; padding: 0; width: fit-content; }
  .st-form, .st-detail { display: flex; flex-direction: column; gap: 12px; max-width: 720px; }
  .st-field { display: flex; flex-direction: column; gap: 4px; font-size: 13px; }
  .st-field-label { font-size: 13px; }
  .st-field input, .st-field textarea, .st-field select { background: color-mix(in srgb, currentColor 6%, transparent); border: 1px solid color-mix(in srgb, currentColor 14%, transparent); border-radius: 8px; padding: 8px 10px; color: inherit; font: inherit; }
  .st-form-ops { display: flex; gap: 8px; }
  .st-sec { display: flex; flex-direction: column; gap: 8px; }
  .st-pls { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .st-pl { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, currentColor 5%, transparent); }
  .st-pl-swatch { width: 10px; height: 10px; border-radius: 3px; flex: none; }
  .st-pl-name { font-weight: 600; flex: 1; }
  /* 周×24h 网格 */
  .st-grid { display: grid; grid-template-columns: 44px repeat(7, 1fr); gap: 2px; max-width: 720px; }
  .st-grid-day { text-align: center; font-size: 11px; opacity: 0.7; padding: 2px 0; }
  .st-grid-hour { font-size: 10px; opacity: 0.5; align-self: center; text-align: right; padding-right: 4px; }
  .st-cell { aspect-ratio: 1.6; border: 1px solid color-mix(in srgb, currentColor 10%, transparent); border-radius: 3px; background: color-mix(in srgb, currentColor 4%, transparent); cursor: pointer; padding: 0; min-width: 0; }
  .st-cell:hover { border-color: color-mix(in srgb, currentColor 40%, transparent); }
  .st-cell.filled { border: none; opacity: 0.85; }
  .st-cell.filled:hover { opacity: 1; }
  /* 对话框 */
  .st-dlg { position: fixed; inset: 0; background: rgb(0 0 0 / 45%); display: grid; place-items: center; z-index: 60; }
  .st-dlg-body { background: var(--surface, #1a1a24); color: inherit; border-radius: 14px; padding: 20px; width: min(480px, 92vw); max-height: 84vh; overflow: auto; display: flex; flex-direction: column; gap: 10px; }
  .st-row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .st-days { display: flex; gap: 6px; flex-wrap: wrap; }
  .st-day { padding: 5px 12px; border-radius: 8px; border: 1px solid color-mix(in srgb, currentColor 18%, transparent); background: none; color: inherit; cursor: pointer; font: inherit; font-size: 12px; }
  .st-day.on { background: var(--accent, #7aa2f7); border-color: transparent; color: #10101a; }
  .st-hits { list-style: none; margin: 0; padding: 0; max-height: 180px; overflow: auto; display: flex; flex-direction: column; gap: 4px; }
  .st-hit { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; }
</style>
