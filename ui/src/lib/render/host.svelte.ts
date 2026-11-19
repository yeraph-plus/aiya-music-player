import { getContext, setContext } from 'svelte';
import type { SpectrumStyle } from '../settings';
import type { SpectrumSource } from '../spectrum-source';

export interface ForegroundRequest {
  target: HTMLElement;
  source: SpectrumSource;
  style: SpectrumStyle;
  paused: boolean;
}

const KEY = Symbol('spectrum-stage');

/** 舞台宿主的页面登记接口，不持有业务状态或额外音频源。 */
export function provideRenderHost() {
  const host = $state<{ foreground: ForegroundRequest | null }>({ foreground: null });
  setContext(KEY, host);
  return host;
}

export type RenderHost = ReturnType<typeof provideRenderHost>;
export function useRenderHost(): RenderHost { return getContext<RenderHost>(KEY); }
