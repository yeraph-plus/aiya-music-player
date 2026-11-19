import { expect, test, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

const SHAPES = ['orbit', 'ribbon', 'lattice', 'contour', 'string', 'shore'];
const errors = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  const messages: string[] = []; errors.set(page, messages);
  page.on('pageerror', error => messages.push(error.message));
  page.on('console', message => { if (message.type() === 'error') messages.push(message.text()); });
});
test.afterEach(({ page }) => expect(errors.get(page)).toEqual([]));

test('六前景设置与非法值回默认，中英文标签完整', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=settings&lang=en&fg=shore');
  const select = page.getByRole('combobox', { name: 'Spectrum style', exact: true });
  await expect(select).toHaveText('Twin Shores'); await select.click();
  await expect(page.getByRole('option')).toHaveText(['Orbital Halo', 'Spectral Ribbon', 'Rhythm Lattice', 'Echo Contours', 'Drifting String', 'Twin Shores']);
  const styles = await page.evaluate(async () => {
    const path = '/src/lib/settings-migrate.ts', schema = '/src/lib/ipc/schema.ts';
    const { mergeSettings } = await import(path), { settingsSchema } = await import(schema);
    return ['orbit','ribbon','lattice','contour','string','shore','mirror','radial','ripple','wave','off','bad'].map(spectrumStyle => {
      const raw = { spectrumStyle, npSpectrum: false, language: 'en' }, value = mergeSettings(raw);
      return [value.spectrumStyle, value.npSpectrum, value.language, settingsSchema.safeParse(raw).success];
    });
  });
  expect(styles).toEqual([...SHAPES,...Array(6).fill('orbit')].map(value => [value,false,'en',true]));
  await page.goto('/?backend=mock&page=settings&fg=shore');
  await page.getByRole('combobox', { name: '频谱形态', exact: true }).click();
  await expect(page.getByRole('option')).toHaveText(['轨道光环','声纹丝带','韵律晶格','回声轮廓','游弦单线','双岸声场']);
});

test('六 shader 实际输出、暂停冻结、静音与断流，连续切换只保留当前资源', async ({ page }, info) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?backend=mock&page=home');
  const results = await page.evaluate(async shapes => {
    const path = '/src/lib/render/context.ts', fgPath = '/src/lib/render/layers/foreground.ts';
    const { GlStage } = await import(path), { createForeground } = await import(fgPath);
    const canvas = document.createElement('canvas'); canvas.style.cssText = 'position:fixed;inset:0;width:640px;height:360px';
    document.body.append(canvas); const stage = new GlStage(canvas), gl = stage.renderer.getContext();
    const bands = new Float32Array(48).fill(.6), source = { bandCount:48,live:true,start(){},stop(){},frame:()=>bands };
    let now = 100, layer: ReturnType<typeof createForeground> | null = null;
    const pixels = new Uint8Array(640*360*4), output: object[] = [];
    const sample = () => {
      stage.render(now += 1000/60); gl.readPixels(0,0,640,360,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
      let sum=0, count=0; for(let i=3;i<pixels.length;i+=4) { sum+=pixels[i]; if(pixels[i]>3) count++; }
      return { sum, count };
    };
    for (let i=0;i<24;i++) {
      if(layer) stage.remove(layer);
      const shape=shapes[i%shapes.length]; layer=createForeground(source,shape); stage.add(layer);
      source.live=true; bands.fill(.6);
      for(let f=0;f<120;f++) stage.render(now+=1000/60);
      const active=sample(), time=Reflect.get(layer,'time');
      const memory={...stage.renderer.info.memory,programs:stage.renderer.info.programs?.length};
      if(i<6) {
        layer.setPaused(true); for(let f=0;f<240;f++) stage.render(now+=1000/60);
        const stopped=sample(), frozen=Reflect.get(layer,'time')===time;
        layer.setPaused(false); source.live=false;
        for(let f=0;f<120;f++) stage.render(now+=1000/60);
        const absent=sample();
        source.live=true; bands.fill(0); for(let f=0;f<300;f++) stage.render(now+=1000/60);
        const quietTime=Reflect.get(layer,'time'); for(let f=0;f<60;f++) stage.render(now+=1000/60);
        output.push({shape,active,stopped,frozen,absent,quietFrozen:Reflect.get(layer,'time')===quietTime,memory});
      } else output.push({shape,memory});
    }
    stage.dispose(); canvas.remove();
    return { output, released:{...stage.renderer.info.memory,programs:stage.renderer.info.programs?.length} };
  }, SHAPES);
  await writeFile(info.outputPath('foreground-gl.json'),JSON.stringify(results,null,2));
  for (const row of results.output as { active?:{sum:number;count:number}; stopped?:{sum:number}; absent?:{sum:number}; frozen?:boolean;quietFrozen?:boolean;memory:object }[]) {
    expect(row.memory).toEqual({ geometries:1,textures:1,programs:1 });
    if(row.active) {
      expect(row.active.count).toBeGreaterThan(200); expect(row.stopped!.sum).toBe(0); expect(row.absent!.sum).toBe(0);
      expect(row.frozen && row.quietFrozen).toBe(true);
    }
  }
  expect(results.released).toEqual({geometries:0,textures:0,programs:0});
});

test('产品共用一个 GL 画布，背景和前景独立开关、暂停、路由与上下文恢复', async ({ page }, info) => {
  await page.goto('/?backend=mock&page=nowPlaying&bg=aurora&fg=string');
  await page.getByRole('button',{name:'播放',exact:true}).click();
  const snapshot = () => page.evaluate(async () => {
    const playerPath='/src/stores/player.svelte.ts', schedule='/src/lib/render/scheduler.ts';
    const { player }=await import(playerPath),{renderTickCount}=await import(schedule);
    return {canvas:document.querySelectorAll('.ambient-bg canvas').length,refs:Reflect.get(player.getSpectrumSource(),'refs'),ticks:renderTickCount()};
  });
  await expect.poll(snapshot).toEqual({canvas:1,refs:2,ticks:2});
  const original=await page.locator('.ambient-bg canvas').elementHandle();
  await page.evaluate(async () => {
    const path='/src/lib/render/context.ts';const {GlStage}=await import(path), render=GlStage.prototype.render;
    GlStage.prototype.render=function(now:number) { Reflect.set(window,'b2Stage',this); return render.call(this,now); };
  });
  await expect.poll(()=>page.evaluate(()=>Boolean(Reflect.get(window,'b2Stage')))).toBe(true);
  const memory = () => page.evaluate(() => {
    const stage=Reflect.get(window,'b2Stage'); return {...stage.renderer.info.memory,programs:stage.renderer.info.programs.length};
  });
  await expect.poll(memory).toEqual({geometries:3,textures:3,programs:4});
  for (const style of SHAPES) {
    await page.evaluate(async style => {const path='/src/stores/ui.svelte.ts';const {ui}=await import(path);ui.settings={...ui.settings,spectrumStyle:style};},style);
    await expect.poll(memory).toEqual({geometries:3,textures:3,programs:4});
    await expect.poll(snapshot).toEqual({canvas:1,refs:2,ticks:2});
  }
  await page.getByRole('button',{name:'暂停',exact:true}).click();
  await expect.poll(snapshot).toEqual({canvas:1,refs:2,ticks:1});
  await page.evaluate(async () => {const path='/src/stores/ui.svelte.ts';const {ui}=await import(path);ui.settings={...ui.settings,bgSpectrum:false};});
  await expect.poll(memory).toEqual({geometries:1,textures:1,programs:1});
  await expect.poll(snapshot).toEqual({canvas:1,refs:1,ticks:1});
  expect(await original!.evaluate(node=>node===document.querySelector('.ambient-bg canvas'))).toBe(true);
  await page.evaluate(async () => {
    const canvas=document.querySelector('.ambient-bg canvas') as HTMLCanvasElement;
    const gl=canvas.getContext('webgl2')!,ext=gl.getExtension('WEBGL_lose_context')!;
    Reflect.set(window,'b2Restore',()=>ext.restoreContext());
    const done=new Promise(resolve=>canvas.addEventListener('webglcontextlost',resolve,{once:true}));ext.loseContext();await done;
  });
  await expect.poll(snapshot).toEqual({canvas:1,refs:0,ticks:1});
  await expect(page.locator('.ambient-flow')).toHaveCSS('visibility','hidden');
  await page.waitForTimeout(100); await page.evaluate(()=>Reflect.get(window,'b2Restore')());
  await expect(page.locator('.ambient-flow')).toHaveCSS('visibility','visible');
  await expect.poll(snapshot).toEqual({canvas:1,refs:1,ticks:1});
  await page.emulateMedia({reducedMotion:'reduce'});
  await expect.poll(snapshot).toEqual({canvas:0,refs:0,ticks:0});
  await page.emulateMedia({reducedMotion:'no-preference'});
  await expect.poll(snapshot).toEqual({canvas:1,refs:1,ticks:1});
  await page.evaluate(async () => {const path='/src/stores/ui.svelte.ts';const {ui}=await import(path);ui.settings={...ui.settings,bgSpectrum:true,npSpectrum:false};});
  await expect.poll(snapshot).toEqual({canvas:1,refs:1,ticks:1});
  await expect.poll(memory).toEqual({geometries:2,textures:2,programs:3});
  await page.evaluate(async () => {const path='/src/stores/ui.svelte.ts';const {ui}=await import(path);ui.settings={...ui.settings,npSpectrum:true};});
  await expect.poll(snapshot).toEqual({canvas:1,refs:2,ticks:1});
  await page.keyboard.press('Escape');
  await expect(page.locator('.now-playing-page')).toHaveCount(0);
  await expect.poll(snapshot).toEqual({canvas:1,refs:1,ticks:1});
  await page.evaluate(async () => {const path='/src/stores/ui.svelte.ts';const {ui}=await import(path);ui.settings={...ui.settings,glEffects:false};});
  await expect.poll(snapshot).toEqual({canvas:0,refs:0,ticks:0});
  await writeFile(info.outputPath('product-lifecycle.json'),JSON.stringify({final:await snapshot(),released:await memory()},null,2));
  expect(await memory()).toEqual({geometries:0,textures:0,programs:0});
});

test('无硬件 GL 的播放页保持静态，既不订阅 tap 也不绘制', async ({page}) => {
  await page.addInitScript(()=>{
    const original=HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext=function(kind:string,...args:unknown[]) {
      if(kind==='webgl2') return null;
      return Reflect.apply(original,this,[kind,...args]);
    } as typeof original;
  });
  await page.goto('/?backend=mock&page=nowPlaying&bg=off&fg=orbit');
  await expect(page.locator('.np-spectrum')).toHaveCount(1);
  await expect(page.locator('.ambient-bg canvas')).toHaveCount(0);
  const refs=await page.evaluate(async()=>{const path='/src/stores/player.svelte.ts';return Reflect.get((await import(path)).player.getSpectrumSource(),'refs');});
  expect(refs).toBe(0);
  await expect(page.locator('.lyrics-panel')).toBeVisible();
});

test('18 种前后景组合明暗截图，以及六种独立前景', async ({page},info) => {
  test.setTimeout(120_000);
  for(const bg of ['ridge','aurora','curtain','off']) {
    for(const fg of SHAPES) {
      for(const theme of ['dark','light']) {
        await page.goto(`/?backend=mock&page=nowPlaying&bg=${bg}&fg=${fg}&theme=${theme}`);
        await page.getByRole('button',{name:'播放',exact:true}).click();
        await expect(page.locator('.ambient-bg canvas')).toHaveCount(1);
        await expect(page.locator('.np-spectrum canvas')).toHaveCount(0);
        await page.waitForTimeout(750);
        await page.screenshot({path:info.outputPath(`${bg}-${fg}-${theme}.png`)});
      }
    }
  }
});

for (const dpr of [1,2]) test.describe(`区域 DPR ${dpr}`, () => {
  test.use({deviceScaleFactor:dpr});
  for (const zoom of [1,1.25,1.5]) test(`CSS 缩放 ${zoom} 的 viewport/scissor 与祖先裁切`, async ({page},info) => {
    await page.emulateMedia({reducedMotion:'reduce'});await page.goto('/?backend=mock&page=home');
    const result=await page.evaluate(async zoom=>{
      const context='/src/lib/render/context.ts',three='/node_modules/.vite/deps/three.js';
      const {GlStage}=await import(context),{Mesh,Scene,PlaneGeometry,OrthographicCamera,ShaderMaterial}=await import(three);
      const canvas=document.createElement('canvas'),clip=document.createElement('div'),target=document.createElement('div');
      canvas.style.cssText='position:fixed;left:0;top:0;width:640px;height:400px';
      clip.style.cssText='position:fixed;left:100px;top:80px;width:250px;height:200px;overflow:hidden';
      target.style.cssText='position:absolute;left:20px;top:30px;width:320px;height:180px';
      document.documentElement.style.zoom=String(zoom);clip.append(target);document.body.append(canvas,clip);
      const stage=new GlStage(canvas),gl=stage.renderer.getContext(),geometry=new PlaneGeometry(2,2),camera=new OrthographicCamera(-1,1,1,-1,0,1);
      const scene=new Scene(),material=new ShaderMaterial({vertexShader:'void main(){gl_Position=vec4(position.xy,0.,1.);}',fragmentShader:'void main(){gl_FragColor=vec4(1.,0.,0.,1.);}'});
      scene.add(new Mesh(geometry,material));
      let width=0,height=0;
      stage.add({name:'solid-region',resize(w:number,h:number){width=w;height=h;},draw(s:typeof stage){s.renderer.render(scene,camera);},dispose(){geometry.dispose();material.dispose();}},target);
      await new Promise(requestAnimationFrame);stage.render(100);
      const pixel=new Uint8Array(4),sample=(x:number,y:number)=>{
        gl.readPixels(Math.floor(x*devicePixelRatio),Math.floor((400-y)*devicePixelRatio),1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);return [...pixel];
      };
      const inside=sample(160,140),outsideClip=sample(380,140),above=sample(160,90);
      clip.style.left='140px';clip.style.width='180px';
      await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);stage.render(116);
      const movedInside=sample(180,140),movedOutside=sample(340,140);
      const viewport=[...gl.getParameter(gl.VIEWPORT)],scissor=gl.isEnabled(gl.SCISSOR_TEST);
      const value={inside,outsideClip,above,movedInside,movedOutside,viewport,scissor,width,height};
      stage.dispose();canvas.remove();clip.remove();document.documentElement.style.zoom='';return value;
    },zoom);
    expect(result.inside).toEqual([255,0,0,255]);expect(result.movedInside).toEqual([255,0,0,255]);
    expect(result.outsideClip[3]).toBe(0);expect(result.above[3]).toBe(0);expect(result.movedOutside[3]).toBe(0);
    expect(result.viewport).toEqual([0,0,640*dpr,400*dpr]);expect(result.scissor).toBe(false);
    expect(result.width).toBeCloseTo(320);expect(result.height).toBeCloseTo(180);
    await writeFile(info.outputPath('region.json'),JSON.stringify(result,null,2));
  });
});
