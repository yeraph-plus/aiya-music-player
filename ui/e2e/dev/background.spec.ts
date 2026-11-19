import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

test.beforeEach(({ page }) => {
  page.on('pageerror', error => { throw error; });
  page.on('console', message => {
    if (message.type() === 'error') throw new Error(message.text());
  });
});

test('平滑核抑制孤立峰，保留宽能量，帧率无关且复用缓冲', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=home');
  const result = await page.evaluate(async () => {
    const path = '/src/lib/render/soft-spectrum.ts';
    const { SoftSpectrum } = await import(path);
    const input = new Float32Array(48), spike = new SoftSpectrum();
    input[24] = 1;
    const buffer = spike.values;
    for (let i = 0; i < 180; i++) spike.update(input, 1 / 60);
    const peak = Math.max(...buffer);
    let slope = 0;
    for (let i = 1; i < buffer.length; i++) slope = Math.max(slope, Math.abs(buffer[i] - buffer[i - 1]));
    input.fill(1);
    const a = new SoftSpectrum(), b = new SoftSpectrum();
    for (let i = 0; i < 60; i++) a.update(input, 1 / 60);
    for (let i = 0; i < 30; i++) b.update(input, 1 / 30);
    const uniform = a.values[128];
    const rateDiff = Math.max(...a.values.map((v: number, i: number) => Math.abs(v - b.values[i])));
    input.fill(0);
    for (let i = 0; i < 240; i++) a.update(input, 1 / 60);
    const silence = Math.max(...a.values);
    input.fill(NaN); input[0] = Infinity; input[1] = -5; input[2] = 5;
    spike.update(input, NaN); spike.update(new Float32Array(), .1);
    return { peak, slope, uniform, rateDiff, silence, reused: buffer === spike.values,
      finite: [...buffer].every(v => Number.isFinite(v) && v >= 0 && v <= 1) };
  });
  expect(result.peak).toBeLessThan(.14);
  expect(result.peak).toBeGreaterThan(.1);
  expect(result.slope).toBeLessThan(.008);
  expect(result.uniform).toBeGreaterThan(.98);
  expect(result.rateDiff).toBeLessThan(.00001);
  expect(result.silence).toBeLessThan(.00001);
  expect(result.reused && result.finite).toBe(true);
});

test('背景设置只接受三种形态，非法形态落默认并保留其他设置', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=settings&lang=en&bg=curtain');
  await expect(page.getByRole('combobox', { name: 'Background spectrum style', exact: true })).toHaveText('Diffuse Curtain');
  await expect(page.getByRole('radiogroup', { name: 'Background spectrum glow', exact: true })).toHaveCount(0);
  const values = await page.evaluate(async () => {
    const path = '/src/lib/settings-migrate.ts', schemaPath = '/src/lib/ipc/schema.ts';
    const { mergeSettings } = await import(path);
    const { settingsSchema } = await import(schemaPath);
    return ['ridge', 'aurora', 'curtain', 'ripple', 'invalid'].map(bgSpectrumStyle => {
      const raw = { bgSpectrumStyle, language: 'en', uiScale: 125 };
      const merged = mergeSettings(raw);
      return [merged.bgSpectrumStyle, merged.language, merged.uiScale, settingsSchema.safeParse(raw).success];
    });
  });
  expect(values).toEqual(['ridge', 'aurora', 'curtain', 'ridge', 'ridge'].map(value => [value, 'en', 125, true]));
});

test('实时数据以帧到达为准，引用计数与销毁正确关停 tap', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=home');
  const result = await page.evaluate(async () => {
    const path = '/src/lib/spectrum-source.ts', clientPath = '/src/lib/ipc/client.ts', eventsPath = '/src/lib/ipc/events.ts';
    const { RemoteSpectrumSource } = await import(path);
    const { ipc } = await import(clientPath);
    const { events } = await import(eventsPath);
    const calls: boolean[] = [];
    const set = ipc.app.spectrumSet, subscribe = events.spectrumFrame;
    let emit: (values: number[]) => void = () => {};
    let removed = 0;
    ipc.app.spectrumSet = async (on: boolean) => { calls.push(on); };
    events.spectrumFrame = (cb: (values: number[]) => void) => { emit = cb; return () => removed++; };
    const source = new RemoteSpectrumSource();
    try {
      source.start(); source.start();
      const before = source.live;
      emit(new Array(48).fill(255));
      const active = source.live && source.frame()[47] === 1;
      emit([64]);
      const shortCleared = source.frame()[47] === 0;
      source.stop();
      const retained = source.live;
      await new Promise(resolve => setTimeout(resolve, 1050));
      const stale = source.live;
      source.start(); source.dispose(); source.start();
      return { before, active, shortCleared, retained, stale, calls, removed, cleared: source.frame().every((v: number) => v === 0) };
    } finally {
      ipc.app.spectrumSet = set; events.spectrumFrame = subscribe;
    }
  });
  expect(result).toEqual({ before: false, active: true, shortCleared: true, retained: true, stale: false,
    calls: [true, false], removed: 1, cleared: true });
});

test('三背景真 GL 绘制、暂停与断流、20 次切换和恢复释放资源', async ({ page }, info) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=home');
  const result = await page.evaluate(async () => {
    const contextPath = '/src/lib/render/context.ts', ridgePath = '/src/lib/render/layers/ridge.ts';
    const atmospherePath = '/src/lib/render/layers/atmosphere.ts', flowPath = '/src/lib/render/layers/flow.ts';
    const { GlStage } = await import(contextPath), { RidgeLayer } = await import(ridgePath);
    const { AtmosphereLayer } = await import(atmospherePath), { FlowLayer } = await import(flowPath);
    const canvas = document.createElement('canvas');
    canvas.style.cssText = 'position:fixed;width:640px;height:360px;inset:0;z-index:9999';
    document.body.append(canvas);
    const stage = new GlStage(canvas), renderer = stage.renderer, gl = renderer.getContext();
    const debug = gl.getExtension('WEBGL_debug_renderer_info');
    const gpu = debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    const bands = new Float32Array(48);
    for (let i = 0; i < 48; i++) bands[i] = .15 + .6 * Math.exp(-(((i - 12) / 10) ** 2));
    const source = { bandCount: 48, live: true, start() {}, stop() {}, frame: () => bands };
    const flow = new FlowLayer(); flow.setPaused(true); stage.add(flow);
    let now = 100, layer: InstanceType<typeof RidgeLayer> | InstanceType<typeof AtmosphereLayer> | null = null;
    const pixels = new Uint8Array(640 * 360 * 4);
    const capture = () => {
      stage.render(now += 1000 / 60);
      gl.readPixels(0, 0, 640, 360, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      let sum = 0, nonzero = 0;
      for (let i = 3; i < pixels.length; i += 4) { sum += pixels[i]; if (pixels[i] > 3) nonzero++; }
      return { sum, nonzero };
    };
    const resting = capture().sum;
    const draws: object[] = [], memory: object[] = [];
    for (let i = 0; i < 21; i++) {
      if (layer) stage.remove(layer);
      const kind = ['ridge', 'aurora', 'curtain'][i % 3];
      layer = kind === 'ridge' ? new RidgeLayer(source, 'soft') : new AtmosphereLayer(source, kind);
      stage.add(layer);
      for (let f = 0; f < 60; f++) stage.render(now += 1000 / 60);
      const active = capture();
      memory.push({ ...renderer.info.memory, programs: renderer.info.programs?.length });
      if (i < 3) {
        layer.setPaused(true);
        const timeBefore = Reflect.get(layer, 'time');
        for (let f = 0; f < 240; f++) stage.render(now += 1000 / 60);
        const paused = capture().sum, frozen = Reflect.get(layer, 'time') === timeBefore;
        layer.setPaused(false); source.live = false;
        for (let f = 0; f < 120; f++) stage.render(now += 1000 / 60);
        draws.push({ kind, active, resting, paused, frozen, absent: capture().sum });
        source.live = true;
      }
    }
    const extension = gl.getExtension('WEBGL_lose_context')!;
    const lost = new Promise<void>(resolve => canvas.addEventListener('webglcontextlost', () => resolve(), { once: true }));
    extension.loseContext(); await lost;
    const unavailable = !stage.usable;
    canvas.style.height = '420px';
    const restored = new Promise<void>(resolve => canvas.addEventListener('webglcontextrestored', () => resolve(), { once: true }));
    await new Promise(resolve => setTimeout(resolve, 100));
    extension.restoreContext(); await restored;
    for (let f = 0; f < 60; f++) stage.render(now += 1000 / 60);
    const recovered = stage.usable && canvas.height === Math.round(420 * Math.min(2, devicePixelRatio));
    const frameTimes: number[] = [];
    const clockLayer = { name: 'clock-test', draw: (_stage: unknown, frame: { time: number }) => frameTimes.push(frame.time), dispose() {} };
    stage.add(clockLayer); stage.render(now += 16);
    document.dispatchEvent(new Event('visibilitychange'));
    stage.render(now += 60_000);
    const noJump = frameTimes[0] === frameTimes[1];
    stage.dispose(); stage.remove(layer!); stage.dispose();
    const released = { ...renderer.info.memory, programs: renderer.info.programs?.length };
    canvas.remove();
    return { gpu, browser: navigator.userAgent, dpr: devicePixelRatio, draws, memory, unavailable, recovered, noJump, released };
  });
  await info.attach('background-gl.json', { body: JSON.stringify(result, null, 2), contentType: 'application/json' });
  await writeFile(info.outputPath('background-gl.json'), JSON.stringify(result, null, 2));
  for (const draw of result.draws as { active: {sum:number;nonzero:number}; resting:number; paused:number; frozen:boolean; absent:number }[]) {
    expect(draw.active.nonzero).toBeGreaterThan(1000);
    expect(draw.active.sum).toBeGreaterThan(draw.resting + 5000);
    expect(Math.abs(draw.paused - draw.resting)).toBeLessThan(500);
    expect(Math.abs(draw.absent - draw.resting)).toBeLessThan(500);
    expect(draw.frozen).toBe(true);
  }
  expect(result.memory.every((m: { geometries:number; textures:number; programs:number }) => m.geometries === 2 && m.textures === 2 && m.programs === 3)).toBe(true);
  expect(result.unavailable && result.recovered && result.noJump).toBe(true);
  expect(result.released).toEqual({ geometries: 0, textures: 0, programs: 0 });
});

test('产品背景切换共用舞台与调度，关闭、reduced motion 与无 GL 静态降级', async ({ page }, info) => {
  await page.goto('/?backend=mock&page=home&track=1&bg=aurora');
  await page.getByRole('button', { name: '播放', exact: true }).click();
  const result = await page.evaluate(async () => {
    const path = '/src/lib/render/capability.ts', schedulePath = '/src/lib/render/scheduler.ts';
    const { detectCapability } = await import(path), { renderTickCount } = await import(schedulePath);
    return { capability: detectCapability(), ticks: renderTickCount() };
  });
  await info.attach('product-capability.json', { body: JSON.stringify(result), contentType: 'application/json' });
  await writeFile(info.outputPath('product-capability.json'), JSON.stringify(result, null, 2));
  if (result.capability.tier < 2) {
    await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
    expect(result.ticks).toBe(0);
    return;
  }
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(1);
  const original = await page.locator('.ambient-bg canvas').elementHandle();
  await page.evaluate(async () => {
    const path = '/src/stores/ui.svelte.ts';
    const { ui } = await import(path);
    for (let i = 0; i < 20; i++) {
      ui.settings = { ...ui.settings, bgSpectrumStyle: ['ridge', 'aurora', 'curtain'][i % 3] };
      await new Promise(resolve => requestAnimationFrame(resolve));
    }
  });
  expect(await original!.evaluate(node => node === document.querySelector('.ambient-bg canvas'))).toBe(true);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(1);
  await page.evaluate(async () => {
    const path = '/src/stores/ui.svelte.ts';
    const { ui } = await import(path); ui.settings = { ...ui.settings, glEffects: false };
  });
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
});

test('硬件 WebGL 不可用时保留 CSS 背景，不订阅背景绘制', async ({ page }) => {
  await page.addInitScript(() => {
    const getContext = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (kind: string, ...args: unknown[]) {
      if (kind === 'webgl2') return null;
      return Reflect.apply(getContext, this, [kind, ...args]);
    } as typeof getContext;
  });
  await page.goto('/?backend=mock&page=settings&bg=aurora');
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
  await expect(page.locator('.hint-warn')).toHaveText('本机无法使用硬件 WebGL，背景使用静态渐变，播放页频谱停用');
  await expect(page.locator('.ambient-art')).not.toHaveCSS('background-image', 'none');
  const ticks = await page.evaluate(async () => {
    const path = '/src/lib/render/scheduler.ts'; return (await import(path)).renderTickCount();
  });
  expect(ticks).toBe(0);
});

test('背景明暗与五配色组合保留页面内容，导出 30 张取证截图', async ({ page }, info) => {
  test.setTimeout(90_000);
  for (const shape of ['ridge', 'aurora', 'curtain']) {
    for (const theme of ['dark', 'light']) {
      for (const ambient of ['aurora', 'ember', 'meadow', 'dusk', 'mono']) {
        await page.goto(`/?backend=mock&page=home&track=1&bg=${shape}&theme=${theme}&ambient=${ambient}`);
        await page.getByRole('button', { name: '播放', exact: true }).click();
        await expect(page.getByRole('button', { name: '暂停', exact: true })).toBeVisible();
        await expect(page.locator('html')).toHaveAttribute('data-ambient', ambient);
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
        await expect(page.locator('.ambient-bg canvas')).toHaveCount(1);
        // 截图包含至少三倍极光时间常数，避免把初始透明帧当作效果验收。
        await page.waitForTimeout(750);
        await page.screenshot({ path: info.outputPath(`${shape}-${theme}-${ambient}.png`) });
      }
    }
  }
});
