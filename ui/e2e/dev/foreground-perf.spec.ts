import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';

for(const style of ['orbit','ribbon','lattice','contour','string','shore']) {
  test(`${style} 与三背景组合持续绘制取证`,async({page},info)=>{
    test.setTimeout(50_000);
    const errors:string[]=[];
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
    await page.emulateMedia({reducedMotion:'reduce'});
    await page.goto('/?backend=mock&page=nowPlaying');
    await expect(page.locator('.np-spectrum')).toBeAttached();
    const metrics=await page.evaluate(async style=>{
      const context='/src/lib/render/context.ts',fgPath='/src/lib/render/layers/foreground.ts';
      const ridgePath='/src/lib/render/layers/ridge.ts',atmospherePath='/src/lib/render/layers/atmosphere.ts',flowPath='/src/lib/render/layers/flow.ts';
      const {GlStage}=await import(context),{createForeground}=await import(fgPath),{RidgeLayer}=await import(ridgePath);
      const {AtmosphereLayer}=await import(atmospherePath),{FlowLayer}=await import(flowPath);
      const canvas=document.createElement('canvas');canvas.style.cssText='position:fixed;inset:0;width:1280px;height:800px';
      document.body.append(canvas);const stage=new GlStage(canvas),renderer=stage.renderer,gl=renderer.getContext();
      const debug=gl.getExtension('WEBGL_debug_renderer_info'),gpu=gl.getParameter(debug?debug.UNMASKED_RENDERER_WEBGL:gl.RENDERER);
      const bands=new Float32Array(48),source={bandCount:48,live:true,start(){},stop(){},frame:()=>bands};
      const target=document.querySelector('.np-spectrum') as HTMLElement;
      stage.add(new FlowLayer());const fg=createForeground(source,style);stage.add(fg,target);
      let bg:InstanceType<typeof RidgeLayer>|InstanceType<typeof AtmosphereLayer>|null=null,index=-1,previous=0;
      const backgrounds=['ridge','aurora','curtain'];
      const records=backgrounds.map(background=>({background,submit:[] as number[],intervals:[] as number[],calls:[] as number[]}));
      const region=[target.clientWidth,target.clientHeight];let detachedFrames=0;
      renderer.info.autoReset=false;const started=performance.now();
      await new Promise<void>(resolve=>{
        const tick=(now:number)=>{
          const t=(now-started)/1000,current=Math.min(2,Math.floor(t/10));
          if(current!==index) {
            if(bg)stage.remove(bg);index=current;
            bg=current===0?new RidgeLayer(source,'soft'):new AtmosphereLayer(source,backgrounds[current]);stage.add(bg);
          }
          for(let i=0;i<48;i++) {
            const phase=Math.floor(t%10/2);
            bands[i]=phase===4?0:phase===0?.02:phase===1&&i>12||phase===2&&i<30?0:Math.max(0,Math.sin(t*3+i*.2))*.8;
          }
          if(!target.isConnected || !target.clientWidth || !target.clientHeight)detachedFrames++;
          renderer.info.reset();const before=performance.now();stage.render(now);const elapsed=performance.now()-before;
          if(t%10>1) { records[index].submit.push(elapsed);records[index].intervals.push(now-previous);records[index].calls.push(renderer.info.render.calls); }
          previous=now;if(t>=30)resolve();else requestAnimationFrame(tick);
        };requestAnimationFrame(tick);
      });
      const pct=(values:number[],p:number)=>values.sort((a,b)=>a-b)[Math.floor((values.length-1)*p)];
      const memory={...renderer.info.memory,programs:renderer.info.programs?.length};
      const result=records.map(r=>({background:r.background,frames:r.submit.length,
        submitMs:{p50:pct(r.submit,.5),p95:pct(r.submit,.95)},intervalMs:{p50:pct(r.intervals,.5),p95:pct(r.intervals,.95)},
        intervalMaxMs:Math.max(...r.intervals),slowFrames:r.intervals.filter(ms=>ms>50).length,
        drawCalls:{min:Math.min(...r.calls),max:Math.max(...r.calls)}}));
      stage.dispose();canvas.remove();
      return {style,gpu,browser:navigator.userAgent,dpr:devicePixelRatio,viewport:[1280,800],
        region,detachedFrames,duration:(performance.now()-started)/1000,result,memory,
        released:{...renderer.info.memory,programs:renderer.info.programs?.length}};
    },style);
    await writeFile(info.outputPath(`${style}-combinations.json`),JSON.stringify(metrics,null,2));
    expect(errors).toEqual([]);expect(metrics.detachedFrames).toBe(0);
    expect(metrics.region.every(value=>value>0)).toBe(true);expect(metrics.result.every(r=>r.frames>100)).toBe(true);
    expect(metrics.memory).toEqual({geometries:3,textures:3,programs:4});
    expect(metrics.released).toEqual({geometries:0,textures:0,programs:0});
  });
}
