import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

for (const shape of ['ridge', 'aurora', 'curtain']) {
  test(`背景 ${shape} 30 秒绘制取证`, async ({ page }, info) => {
    test.setTimeout(50_000);
    const errors: string[] = [];
    page.on('pageerror', e => errors.push(e.message));
    page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/?backend=mock&page=home');
    const metrics = await page.evaluate(async shape => {
      const stagePath = '/src/lib/render/context.ts', ridgePath = '/src/lib/render/layers/ridge.ts';
      const atmospherePath = '/src/lib/render/layers/atmosphere.ts', flowPath = '/src/lib/render/layers/flow.ts';
      const { GlStage } = await import(stagePath), { RidgeLayer } = await import(ridgePath);
      const { AtmosphereLayer } = await import(atmospherePath), { FlowLayer } = await import(flowPath);
      const canvas = document.createElement('canvas');
      canvas.style.cssText = 'position:fixed;inset:0;width:1280px;height:800px;z-index:9999';
      document.body.append(canvas);
      const stage = new GlStage(canvas), renderer = stage.renderer, gl = renderer.getContext();
      const debug = gl.getExtension('WEBGL_debug_renderer_info');
      const gpu = debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
      const bands = new Float32Array(48);
      const source = { bandCount: 48, live: true, start() {}, stop() {}, frame: () => bands };
      stage.add(new FlowLayer());
      stage.add(shape === 'ridge' ? new RidgeLayer(source, 'soft') : new AtmosphereLayer(source, shape));
      renderer.info.autoReset = false;
      const submit: number[] = [], intervals: number[] = [], calls: number[] = [];
      const start = performance.now(); let previous = start;
      await new Promise<void>(resolve => {
        const tick = (now: number) => {
          const t = (now - start) / 1000;
          for (let i = 0; i < bands.length; i++) {
            // 依次包含低能量、低频、高频、宽谱突变与静音；模拟仅存在于测试。
            const phase = Math.floor(t / 6);
            bands[i] = phase === 4 ? 0 : phase === 0 ? .02 :
              (phase === 1 && i > 12) || (phase === 2 && i < 30) ? 0 :
              Math.max(0, Math.sin(t * 3 + i * .2)) * .8;
          }
          renderer.info.reset();
          const before = performance.now(); stage.render(now);
          const elapsed = performance.now() - before;
          if (t > 1) { submit.push(elapsed); intervals.push(now - previous); calls.push(renderer.info.render.calls); }
          previous = now;
          if (t >= 30) resolve(); else requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      });
      const percentile = (values: number[], p: number) => values.sort((a, b) => a - b)[Math.floor((values.length - 1) * p)];
      const memory = { ...renderer.info.memory, programs: renderer.info.programs?.length };
      stage.dispose(); canvas.remove();
      return { shape, browser: navigator.userAgent, gpu, viewport: [1280, 800], dpr: devicePixelRatio,
        seconds: (performance.now() - start) / 1000, frames: submit.length,
        submitMs: { p50: percentile(submit, .5), p95: percentile(submit, .95) },
        intervalMs: { p50: percentile(intervals, .5), p95: percentile(intervals, .95) },
        drawCalls: { min: Math.min(...calls), max: Math.max(...calls) }, memory,
        released: { ...renderer.info.memory, programs: renderer.info.programs?.length } };
    }, shape);
    await writeFile(info.outputPath(`${shape}-30s.json`), JSON.stringify(metrics, null, 2));
    expect(errors).toEqual([]);
    expect(metrics.frames).toBeGreaterThan(100);
    expect(metrics.memory).toEqual({ geometries: 2, textures: 2, programs: 3 });
    expect(metrics.released).toEqual({ geometries: 0, textures: 0, programs: 0 });
  });
}
