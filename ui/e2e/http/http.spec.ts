import { expect, test, type Page } from '@playwright/test';
import { mkdir, copyFile, writeFile } from 'node:fs/promises';
import { dirname, basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * http 形态 e2e：真 aiya-server + 浏览器引擎的完整链路。
 *
 * 与 mock 套件的分工：mock 锁 UI 装配与桌面形态，这里锁 Web 形态——
 * 登录闸首帧、首启种子根、浏览器解码灰标、`<audio>` 真播放、设置页
 * 隐身清单、会话刷新直进与双语。服务端由 webServer 钩子拉起
 * （tools/http-e2e-server.mjs，8799，口令 e2e-pass）。
 *
 * 断言尽量走结构与 evaluate（DOM 直查），少依赖文案——界面语言跟随
 * 服务端首启写库，测试机之间可能是 zh 或 en；需要文案的用例先点
 * 「中文/English」语言钮固定语向（语言钮文案是语文名，恒定）。
 */

const PASSWORD = 'e2e-pass';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.locator('.gate input[type="password"]').fill(PASSWORD);
  await page.locator('.gate button').click();
  await page.locator('aside nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
}

async function goToLibrary(page: Page): Promise<void> {
  await page.locator('aside button', { hasText: /曲库|Library/ }).click();
  await page.locator('.row').first().waitFor({ state: 'visible', timeout: 10_000 });
}

test('登录闸首帧渲染，口令进闸后应用可用', async ({ page }) => {
  await page.goto('/');
  // 首帧即闸（mount 前的 /auth/state 预检）：主界面不得先闪现
  await page.locator('.gate').waitFor({ state: 'visible', timeout: 10_000 });
  expect(await page.locator('aside nav').count()).toBe(0);
  await page.locator('.gate input[type="password"]').fill(PASSWORD);
  await page.locator('.gate button').click();
  await page.locator('aside nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
});

test('首启种子根已入库，浏览器解不了的容器行打灰', async ({ page }) => {
  await login(page);
  await goToLibrary(page);
  // fixtures 三支（mp3 / ogg / aiff）都在库里：bootstrap_scan 的种子根
  const rowCount = await page.locator('.row').count();
  expect(rowCount).toBeGreaterThanOrEqual(3);
  // AIFF 行打灰（Chromium 解不了），其余行不打灰——灰标只此一家
  const unsupported = await page.locator('.row.unsupported').count();
  expect(unsupported).toBe(1);
});

test('双击播放：浏览器引擎装载并真实出声', async ({ page }) => {
  await page.addInitScript(() => {
    const read = AnalyserNode.prototype.getByteFrequencyData;
    AnalyserNode.prototype.getByteFrequencyData = function (buffer) {
      read.call(this, buffer);
      document.documentElement.dataset.spectrumProbe = String(Math.max(Number(document.documentElement.dataset.spectrumProbe ?? 0), ...buffer));
    };
  });
  await login(page);
  await goToLibrary(page);
  await page.locator('audio').evaluate((audio: HTMLAudioElement) => { audio.loop = true; });
  // 指定一支可播容器（AIFF 行点了会走报错，不选）；曲目无标签，标题=文件名
  await page.locator('.row', { hasText: 'tone-mp3' }).dblclick();
  // <audio> 真装载：src 指向 /audio/ 且确实在走表。轮询带回显诊断——
  // paused/readyState/error 的组合能区分"被 autoplay 拦"与"加载失败"
  const deadline = Date.now() + 15_000;
  let last: unknown = null;
  for (;;) {
    last = await page.evaluate(() => {
      const a = document.querySelector('audio');
      if (!a) return null;
      return {
        playing: !a.paused && a.currentTime > 0,
        paused: a.paused,
        t: a.currentTime,
        ready: a.readyState,
        err: a.error?.code ?? null,
        src: a.src.split('/').slice(-1)[0],
      };
    });
    if (last && (last as { playing: boolean }).playing) break;
    if (Date.now() > deadline) break;
    await page.waitForTimeout(400);
  }
  expect(last, JSON.stringify(last)).toMatchObject({ playing: true });
  // B2 之后渲染路径由硬件档位决定（capability.ts）：档 2 → 播放页申请频谱
  // tap（探针有数据）且背景恰好 1 canvas；档 1（软件光栅，CI runner 无 GPU）
  // → 不申请 tap、背景走 CSS 静态渐变（canvas 恒 0）。GPU 能力不进断言
  // （11-12 smoke 先例）：测试先在页面里探一次能力再钉契约，不写死档位。
  const tier2 = await page.evaluate(() => {
    const probe = document.createElement('canvas');
    return probe.getContext('webgl2', { failIfMajorPerformanceCaveat: true }) !== null;
  });
  if (tier2) {
    await expect.poll(() => page.locator('html').getAttribute('data-spectrum-probe').then(value => Number(value))).toBeGreaterThan(0);
    await expect(page.locator('.ambient-bg canvas')).toHaveCount(1);
  } else {
    await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
  }
});

test('设置页 web 形态：桌面段落隐身、目录模态框与单次扫描', async ({ page }) => {
  await login(page);
  await page.locator('aside button', { hasText: /设置|Settings/ }).click();
  // 固定中文语向（语言钮是语文名，两种语言下都这么叫）
  await page.getByRole('radio', { name: '中文' }).click();
  // 语言切换是服务端设置 + 本地即时生效：等中文文案可见（自动重试防慢机）
  await expect(page.getByText('最近播放保留')).toBeVisible();
  const shape = await page.evaluate(() => {
    const text = document.body.innerText;
    const labels = [...document.querySelectorAll('button[aria-label]')].map((b) =>
      b.getAttribute('aria-label'),
    );
    return {
      // 桌面专属段落不渲染
      outputHidden: !text.includes('输出模式') && !text.includes('Output mode'),
      sleepHidden: !text.includes('阻止系统休眠') && !text.includes('Prevent sleep'),
      trayHidden: !text.includes('关闭到托盘') && !text.includes('Close to tray'),
      // 窗控三键不渲染
      windowChromeHidden: !labels.some((l) => /最小化|Minimize/.test(l ?? '')),
      // EQ 触发键不渲染（队列键仍在）
      eqHidden: !labels.some((l) => /均衡器|EQ \(/.test(l ?? '')),
      // web 静态说明与路径手输在
      webDecode: text.includes('由浏览器解码播放') || text.includes('Decoded and played by the browser'),
      rootInput: !!document.querySelector<HTMLInputElement>('.roots-actions input'),
    };
  });
  expect(shape).toEqual({
    outputHidden: true,
    sleepHidden: true,
    trayHidden: true,
    windowChromeHidden: true,
    eqHidden: true,
    webDecode: true,
    rootInput: false,
  });
  await page.getByRole('button', { name: '添加目录', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const field = dialog.getByLabel('容器内目录', { exact: true });
  await expect(field).toBeFocused();
  await field.fill('relative');
  await field.press('Enter');
  await expect(dialog).toContainText('请输入绝对目录路径');
  const token = await page.evaluate(() => localStorage.getItem('aiya.authToken'));
  const call = async (cmd: string, data: Record<string, unknown> = {}) => {
    const r = await page.request.post(`/ipc/${cmd}`, { data, headers: { authorization: `Bearer ${token}` } });
    expect(r.ok(), await r.text()).toBeTruthy(); return r.json();
  };
  const roots = await call('library_roots_get');
  await field.fill(roots.roots[0]);
  await dialog.getByRole('button', { name: '添加并追踪', exact: true }).click();
  await expect(dialog).toContainText('该目录已在曲库根里');
  const info = await call('app_info');
  const folder = join(dirname(info.dataDir), 'extra-music');
  await mkdir(folder, { recursive: true });
  await field.fill(join(folder, 'missing'));
  await dialog.getByRole('button', { name: '添加并追踪', exact: true }).click();
  await expect(dialog).toContainText('Folder does not exist');
  let extraScanRequests = 0;
  page.on('request', r => { if (r.url().endsWith('/ipc/library_scan')) extraScanRequests++; });
  await field.fill(folder);
  await dialog.getByRole('button', { name: '添加并追踪', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.root-path', { hasText: folder })).toBeVisible();
  await expect(page.locator('.status-block')).toContainText('上次索引：');
  expect(extraScanRequests).toBe(0);
  await page.getByRole('button', { name: '立即索引', exact: true }).click();
  await expect(page.locator('.status-block')).toContainText('上次索引：');
  await expect(page.locator('.status-block')).not.toContainText(/unknown|异常退出/);
  await expect.poll(async () => (await call('refresh_state')).last?.trigger).toBe('action');
});

test('会话刷新直进 + 双语抽查（English）', async ({ page }) => {
  await login(page);
  // 有效会话刷新：预检带凭据，直接进应用不再落闸
  await page.reload();
  await page.locator('aside nav').waitFor({ state: 'visible', timeout: 15_000 });
  await expect(page.locator('.gate')).toHaveCount(0);
  // 双语抽查：切 English 后设置页曲库组标题是英文
  await page.locator('aside button', { hasText: /设置|Settings/ }).click();
  await page.getByRole('radio', { name: 'English' }).click();
  await expect(page.getByText('Library folders')).toBeVisible();
});

test('推流工作台：全曲库、整卡队列与多时段一次保存', async ({page}) => {
  test.setTimeout(60000);
  await login(page);
  const token=await page.evaluate(()=>localStorage.getItem('aiya.authToken'));
  const call=async(cmd:string,data:Record<string,unknown>={})=>{
    const r=await page.request.post(`/ipc/${cmd}`,{data,headers:{authorization:`Bearer ${token}`}});
    expect(r.ok(),await r.text()).toBeTruthy();return r.json();
  };
  const tracks=await call('library_list',{query:{limit:10}});
  const morning=await call('playlist_create',{name:'Morning fixture',trackIds:[tracks.items[0].id]});
  const night=await call('playlist_create',{name:'Night fixture',trackIds:[tracks.items[1].id]});
  let stationId:string|undefined;
  try {
    await page.locator('aside button',{hasText:/推流|Broadcasting/}).click();
    await expect(page.locator('.broadcast .ch-kicker')).toHaveText('BROADCAST');
    await expect(page.locator('.broadcast h1')).toHaveText(/推流|Broadcasting/);
    await expect(page.locator('.broadcast input[type="search"]')).toHaveCount(0);
    await expect.poll(async()=>(await call('app_get_ui_state')).page).toBe('broadcast');
    await page.reload();
    await page.getByRole('button',{name:/新建频道|New station/}).click();
    const dialog=page.getByRole('dialog');
    await expect(dialog.locator('.k-field')).toHaveCount(4);
    await expect(dialog.locator('select')).toHaveCount(0);
    await expect(dialog.getByRole('combobox')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);expect(await call('station_list')).toEqual([]);
    await page.getByRole('button',{name:/新建频道|New station/}).click();
    await page.getByPlaceholder(/如「深夜 FM」|Midnight FM/).fill('E2E Radio');
    await page.getByLabel(/副标题|Subtitle/).fill('All fields');
    await page.getByLabel(/描述|Description/).fill('Weekly radio');
    await dialog.getByRole('combobox').click();
    await page.getByRole('option',{name:/公开|Public/}).click();
    await dialog.getByRole('button',{name:/^保存$|^Save$/}).click();
    await expect(page.locator('.station-workbench')).toBeVisible();
    stationId=(await call('station_list'))[0].id;
    const detail=await call('station_get',{stationId});
    expect(detail).toMatchObject({subtitle:'All fields',description:'Weekly radio',visibility:'public'});
    await expect(page.locator('.broadcast h1')).toHaveText(/推流|Broadcasting/);
    await expect(page.locator('.broadcast .ch-kicker')).toHaveText('BROADCAST');
    await page.getByRole('button',{name:/添加 全曲库|Add All tracks/,exact:true}).click();
    await page.getByRole('button',{name:/添加 Morning fixture|Add Morning fixture/,exact:true}).click();
    await page.getByRole('button',{name:/Morning fixture 已添加|Morning fixture already added/}).click();
    await expect(page.locator('.queue-list>li')).toHaveCount(2);
    await page.getByRole('button',{name:/添加 Night fixture|Add Night fixture/,exact:true}).click();
    await expect(page.locator('.queue-list>li')).toHaveCount(3);
    await expect(page.locator('.playlist-grid .k-cover')).toHaveCount(3);
    const queue=page.locator('.queue-list>li').nth(2);
    await expect(queue.locator('select')).toHaveCount(0);
    await queue.getByRole('combobox').click();
    await page.getByRole('option',{name:/随机顺序|Shuffle/}).click();
    await queue.getByRole('switch',{name:/^指定时段$|^Scheduled$/}).click();
    await expect(queue.locator('.week-grid')).toBeVisible();
    await expect(queue.locator('input[type="number"]')).toHaveCount(1);
    await queue.getByLabel(/优先级|Priority/,{exact:true}).fill('4');
    const drag=async(d1:number,h1:number,d2:number,h2:number)=>{
      const first=queue.locator(`.week-cell[data-day="${d1}"][data-hour="${h1}"]`);
      const last=queue.locator(`.week-cell[data-day="${d2}"][data-hour="${h2}"]`);
      await first.scrollIntoViewIfNeeded();
      const a=(await first.boundingBox())!,b=(await last.boundingBox())!;
      await page.mouse.move(a.x+a.width/2,a.y+a.height/2);await page.mouse.down();
      await page.mouse.move(a.x+a.width-1,a.y+a.height/2);
      expect(await page.evaluate(({x,y})=>getComputedStyle(document.elementFromPoint(x,y)!).cursor,{x:a.x+a.width-1,y:a.y+a.height/2})).toBe('crosshair');
      await page.mouse.move(b.x+b.width/2,b.y+b.height/2,{steps:8});await page.mouse.up();
    };
    await drag(0,8,1,9);
    await expect(queue.locator('.slot-row')).toHaveCount(1);
    const slot=queue.locator('.slot-row').first();
    await slot.getByLabel(/开始|Start/,{exact:true}).fill('08:30');
    await slot.getByLabel(/结束|End/,{exact:true}).fill('10:15');
    await expect(queue.locator('.time-band[data-slot="0"]')).toHaveCount(2);
    await drag(6,22,6,23);
    await expect(queue.locator('.slot-row')).toHaveCount(2);
    await expect(queue.locator('.slot-row').nth(1).getByLabel(/结束|End/,{exact:true})).toHaveValue('24:00');
    await drag(0,0,6,23);
    await expect(queue.locator('.slot-row')).toHaveCount(3);
    await expect(queue.getByRole('button',{name:/确认时段|Confirm slot|全部安排|All schedules/})).toHaveCount(0);
    expect((await call('station_get',{stationId})).program.items).toEqual([]);
    await page.locator('aside button',{hasText:/直播|Live/}).click();
    await expect(dialog).toContainText(/未保存|Unsaved/);
    await page.getByRole('button',{name:/继续编辑|Keep editing/}).click();
    await page.getByRole('button',{name:/保存配置|Save configuration/}).click();
    await expect(page.getByRole('button',{name:/保存配置|Save configuration/})).toHaveCount(0);
    const program=(await call('station_get',{stationId})).program;
    expect(program.items.map((i:{playlistId:string})=>i.playlistId)).toEqual(['__library__',morning.id,night.id]);
    expect(program.items[2]).toMatchObject({playMode:'shuffle',priority:4,slots:[{startHm:'08:30',endHm:'10:15',daysMask:3},{startHm:'22:00',endHm:'24:00',daysMask:64},{startHm:'00:00',endHm:'24:00',daysMask:127}]});
    await call('playlist_rename',{id:night.id,name:'Night renamed'});
    await expect(queue.locator('.k-card-title')).toContainText('Night renamed',{timeout:10000});
    for(const width of [960,1280,1440]) {
      await page.setViewportSize({width,height:900});
      expect(await page.locator('.broadcast').evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);
    }
    await page.getByRole('button',{name:/编辑频道信息|Edit station details/}).click();
    await expect(dialog.getByLabel(/副标题|Subtitle/)).toHaveValue('All fields');
    await dialog.getByLabel(/副标题|Subtitle/).fill('Updated');
    await dialog.getByRole('button',{name:/^保存$|^Save$/}).click();
    expect((await call('station_get',{stationId})).subtitle).toBe('Updated');
    await page.getByRole('button',{name:/返回列表|Back to list/}).click();
    await expect(page.locator('.station-workbench')).toHaveCount(0);
    const row=page.locator('.station-list article');
    await expect(row.locator('button')).toHaveText(['','','','']);
    await row.getByRole('button',{name:/开播|Go on air/}).hover();
    await expect(page.getByRole('tooltip')).toHaveText(/开播|Go on air/);
    await expect(row.locator('.on-air')).toHaveAttribute('aria-label',/已停播|Stopped/);
    await expect(row.locator('.k-card-title')).toContainText(/E2E Radio（公开）|E2E Radio \(Public\)/);
    await expect(row.getByRole('link')).toHaveAttribute('href',/\/hls\/.*live.m3u8$/);
    await expect(row.locator('button[aria-expanded],.k-menu')).toHaveCount(0);
    await row.getByRole('button',{name:/^配置$|^Configure$/}).click();
    await expect(queue.locator('.slot-row').first().getByLabel(/开始|Start/,{exact:true})).toHaveValue('08:30');
    await page.locator('.workbench-toolbar').getByRole('button',{name:/^删除$|^Delete$/}).click();
    const confirmation = page.getByRole('alertdialog');
    await expect(confirmation).toContainText('E2E Radio');
    await confirmation.getByRole('button',{name:/取消|Cancel/}).click();
    await page.locator('aside button',{hasText:/直播|Live/}).click();
    await expect(page.getByText('E2E Radio').first()).toBeVisible();
    await call('station_delete',{stationId});stationId=undefined;
  } finally {
    if(stationId)await call('station_delete',{stationId});
    await call('playlist_delete',{id:morning.id});await call('playlist_delete',{id:night.id});
  }
});

for (const locale of ['zh', 'en'] as const) test(`推流双语 ${locale}：校验、来源异常与英文窄屏`, async ({ page }) => {
  test.setTimeout(60000);
  await login(page);
  const token = await page.evaluate(() => localStorage.getItem('aiya.authToken'));
  const call = async (cmd: string, data: Record<string, unknown> = {}) => {
    const r = await page.request.post(`/ipc/${cmd}`, { data, headers: { authorization: `Bearer ${token}` } });
    expect(r.ok(), await r.text()).toBeTruthy(); return r.json();
  };
  const zh = locale === 'zh';
  await page.locator('aside button', { hasText: /设置|Settings/ }).click();
  await page.getByRole('radio', { name: zh ? '中文' : 'English', exact: true }).click();
  const tracks = await call('library_list', { query: { limit: 10 } });
  const single = await call('playlist_create', { name: 'I18N single', trackIds: [tracks.items[0].id] });
  const empty = await call('playlist_create', { name: 'I18N empty', trackIds: [] });
  const station = await call('station_create', { name: 'I18N Radio', visibility: 'private' });
  let emptyDeleted = false;
  try {
    await page.locator('aside').getByRole('button', { name: zh ? '推流' : 'Broadcasting', exact: true }).click();
    await expect(page.locator('.broadcast h1')).toHaveText(zh ? '推流' : 'Broadcasting');
    const row = page.locator(`article[data-station="${station.id}"]`);
    await expect(row.locator('.k-card-title')).toHaveText(zh ? 'I18N Radio（私密）' : 'I18N Radio (Private)');
    await row.getByRole('button', { name: zh ? '复制收听地址' : 'Copy stream URL', exact: true }).hover();
    await expect(page.getByRole('tooltip')).toHaveText(zh ? '复制收听地址' : 'Copy stream URL');
    await row.getByRole('button', { name: zh ? '开播' : 'Go on air', exact: true }).click();
    const emptyQueue = zh ? '开播前请先添加系统歌单或全曲库。' : 'Add a playlist or all tracks before broadcasting.';
    await expect(page.getByText(emptyQueue, { exact: true })).toBeVisible();
    await row.getByRole('button', { name: zh ? '配置' : 'Configure', exact: true }).click();
    await expect(page.locator('.program-issues')).toHaveText(emptyQueue);
    await page.getByRole('button', { name: zh ? '编辑频道信息' : 'Edit station details', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByLabel(zh ? '频道名称（必填）' : 'Station name (required)', { exact: true })).toHaveValue('I18N Radio');
    await expect(dialog.getByRole('combobox')).toContainText(zh ? '私密（服务鉴权）' : 'Private (server authentication)');
    await page.keyboard.press('Escape');
    const source = page.locator('.playlist-grid .k-card').filter({ hasText: 'I18N single' });
    await expect(source.locator('.k-card-sub')).toHaveText(zh ? '手动 · 1 首' : 'Manual · 1 track');
    await expect(page.locator('.playlist-grid .k-card').filter({ hasText: 'I18N empty' }).locator('.k-card-sub')).toHaveText(zh ? '手动 · 0 首' : 'Manual · 0 tracks');
    let revision = (await call('station_get', { stationId: station.id })).program.revision;
    const setItems = async (items: unknown[]) => {
      const program = await call('station_set_program', { stationId: station.id, revision, items });
      revision = program.revision; return program;
    };
    const regular = (playlistId: string) => ({ playlistId, playMode: 'ordered', priority: 0, scheduled: false, slots: [] });
    await setItems([regular(empty.id)]);
    await expect(page.locator('.program-issues')).toHaveText(zh ? '歌单没有可播放曲目：I18N empty' : 'No playable tracks in playlist: I18N empty', { timeout: 10000 });
    await call('playlist_delete', { id: empty.id }); emptyDeleted = true;
    await expect(page.locator('.program-issues')).toHaveText(zh ? `歌单已不存在：${empty.id}` : `Playlist no longer exists: ${empty.id}`, { timeout: 10000 });
    const program = await setItems([{ ...regular(single.id), scheduled: true, slots: [{ startHm: '08:00', endHm: '10:00', daysMask: 1 }] }]);
    expect(program.timezone).toBeNull();
    expect(program.issues).toEqual([{ kind: 'uncoveredTime', weekday: 0, time: '00:00' }]);
    await expect(page.locator('.program-issues')).toHaveText(zh ? '周一 00:00 未安排节目，请添加常规轮播歌单或补全时段。' : 'No program at Mon 00:00. Add a regular playlist or complete the schedule.', { timeout: 10000 });
    await expect(page.locator('.week-head')).toContainText(zh ? '服务器本地时区' : 'Server local time zone');
    await expect(page.locator('.week-grid').getByRole('button', { name: zh ? '周一 08:00–09:00' : 'Mon 08:00–09:00', exact: true })).toHaveCount(1);
    await page.locator('.slot-row').getByLabel(zh ? '结束' : 'End', { exact: true }).fill('25:00');
    await expect(page.locator('.queue-list .program-error')).toHaveText(zh ? '请选择星期，并填写有效且不相同的起止时间（HH:MM）' : 'Choose weekdays and valid, different start/end times (HH:MM).');
    await expect(page.getByRole('button', { name: zh ? '保存配置' : 'Save configuration', exact: true })).toBeDisabled();
    await page.locator('.slot-row').getByLabel(zh ? '结束' : 'End', { exact: true }).fill('10:15');
    await page.setViewportSize({ width: 960, height: 1000 });
    for (const selector of ['.broadcast', '.workbench-toolbar', '.workbench-save', '.slot-line']) {
      expect(await page.locator(selector).evaluate(el => el.scrollWidth <= el.clientWidth + 1), selector).toBe(true);
    }
    await page.locator('.broadcast').evaluate(el => el.scrollTop = 0);
    await page.screenshot({ path: resolve(dirname(fileURLToPath(import.meta.url)), `../../../.aiya-tmp/i18n-${locale}-960.png`), fullPage: true });
    await page.locator('.slot-row').scrollIntoViewIfNeeded();
    await page.screenshot({ path: resolve(dirname(fileURLToPath(import.meta.url)), `../../../.aiya-tmp/i18n-${locale}-schedule-960.png`), fullPage: true });
    await page.getByRole('button', { name: zh ? '取消' : 'Cancel', exact: true }).click();
  } finally {
    await call('station_delete', { stationId: station.id });
    await call('playlist_delete', { id: single.id });
    if (!emptyDeleted) await call('playlist_delete', { id: empty.id });
  }
});

test('私有频道卡片：HLS 装载、分段鉴权和断开重连', async ({ page }) => {
  await login(page);
  const token = await page.evaluate(() => localStorage.getItem('aiya.authToken'));
  const call = async (cmd: string, data: Record<string, unknown> = {}) => {
    const r = await page.request.post(`/ipc/${cmd}`, { data, headers: { authorization: `Bearer ${token}` } });
    expect(r.ok(), await r.text()).toBeTruthy();
    return r.json();
  };
  const summary = await call('station_create', { name: 'Private HLS', visibility: 'private' });
  try {
    const info = await call('app_info');
    const root = dirname(info.dataDir);
    expect(basename(root)).toMatch(/^aiya-http-e2e-/);
    const dir = summary.hlsUrl.split('/')[2];
    expect(dir).toMatch(/^[\w-]+$/);
    const hlsDir = join(root, 'hls', dir);
    await mkdir(hlsDir, { recursive: true });
    await copyFile(resolve(dirname(fileURLToPath(import.meta.url)), '../../../crates/aiya-audio/tests/fixtures/tone-mp3.mp3'), join(hlsDir, 'segment.mp3'));
    await writeFile(join(hlsDir, 'live.m3u8'), '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n' + '#EXTINF:0.627,\nsegment.mp3\n'.repeat(60) + '#EXT-X-ENDLIST\n');
    expect((await page.request.get(summary.hlsUrl)).status()).toBe(401);
    await page.locator('aside button', { hasText: /直播|Live/ }).click();
    await expect(page.locator('.live-card .card-face[aria-label$="Private HLS"]')).toHaveCount(1);
    await expect(page.locator('.live-info h2:not([inert])')).toBeVisible();
    const cardCount = await page.locator('.live-card:not(.add-card)').count();
    for (let i = 0; i < cardCount && await page.locator('.live-info h2:not([inert])').textContent() !== 'Private HLS'; i++) {
      await page.getByRole('button', { name: /下一条流|Next stream/ }).click();
      await expect(page.locator('.stage')).not.toHaveClass(/locked/);
    }
    const card = page.getByRole('button', { name: /播放 Private HLS|Play Private HLS/ });
    const segmentResponse = page.waitForResponse((r) => r.url().includes('/segment.mp3') && r.ok(), { timeout: 8000 });
    await card.click();
    await expect(page.locator('.live-info h2:not([inert])')).toHaveText('Private HLS');
    const segment = await segmentResponse;
    expect(new URL(segment.url()).searchParams.get('token')).toBe(token);
    await expect.poll(() => page.locator('audio').evaluate((a: HTMLAudioElement) => !a.paused && a.currentTime > 0)).toBe(true);
    const before = await page.locator('audio').evaluate((a: HTMLAudioElement) => ({src:a.src,time:a.currentTime}));
    await page.locator('aside button', {hasText:/推流|Broadcasting/}).click();
    await expect(page.locator('.station')).toBeVisible();
    await expect.poll(() => page.locator('audio').evaluate((a: HTMLAudioElement) => !a.paused && a.currentTime)).toBeGreaterThan(before.time);
    await expect(page.locator('audio')).toHaveAttribute('src', before.src);
    await page.locator('aside button', {hasText:/直播|Live/}).click();
    await page.locator('footer').getByRole('button', { name: /断开直播|Disconnect stream/ }).click();
    await expect.poll(() => page.locator('audio').getAttribute('src')).toBeNull();
    await page.locator('footer').getByRole('button', { name: /连接直播|Connect stream/ }).click();
    await expect.poll(() => page.locator('audio').evaluate((a: HTMLAudioElement) => !a.paused && a.currentTime > 0)).toBe(true);
  } finally {
    await call('player_clear');
    await call('station_delete', { stationId: summary.id });
  }
});
