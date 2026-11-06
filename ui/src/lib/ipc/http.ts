/**
 * Web 版 transport（第三实现）+ 浏览器音频引擎。
 *
 * 与 mock 同一替换点（`setTransport`），零架构改动；差异只在两个方向：
 * - 命令 → `POST /ipc/{cmd}`，错误响应体是 ApiError 形状（reject 原样抛出，
 *   `client.ts` 的归一化不变）；
 * - 事件 → SSE `/events` 承载 scan/beets 通道；`player:state` 客户端自产——
 *   命令响应快照为基座、元素时钟合帧 ~10Hz 广播。`spectrum:frame` 与
 *   `audio:mode` 空订阅（前者 W3，后者桌面专属）。
 *
 * 浏览器就是引擎：装载凭 `PlayerState.token` 变化判定（Actor 的装载意图
 * 编号），装载/位置/播完经 `player_report` 回服务端 Actor 归约（Loaded /
 * Position / Ended 与桌面引擎事件一一对应），队列/循环/连播权威在服务端。
 *
 * 凭据：页面 `?token=` 存入模块；命令走 Authorization 头，`<audio>` /
 * EventSource 这类元素级 GET 走查询参数（`withToken`）。
 */
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { PlayerState } from './generated/PlayerState';
import { setTransport, transport, type Transport } from './transport';

let authToken = '';

// 频谱帧订阅（Remote 源引用计数后的唯一回调入口）
const spectrumCbs = new Set<(frame: number[]) => void>();
let spectrumTimer: ReturnType<typeof setInterval> | null = null;

const TOKEN_KEY = 'aiya.authToken';

function readStoredToken(): string {
  try {
    return localStorage.getItem(TOKEN_KEY) ?? '';
  } catch {
    return '';
  }
}

function withToken(url: string): string {
  if (!authToken) return url;
  return url + (url.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(authToken);
}

type StateListener = (s: PlayerState) => void;

class WebPlayerEngine {
  private audio: HTMLAudioElement;
  private listeners = new Set<StateListener>();
  // 频谱旁路：WebAudio AnalyserNode 客户端复刻 48 对数分带（W4 预留件）。
  // MediaElementSource 对同一元素只能建一次；Safari 有静音前科，探测失败
  // 即降级为无频谱（Remote 源保持空态，与桌面 tap 关闭同观感）。
  private ctx: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private spectrumOk = false;
  private spectrumAttempted = false;
  private freqBuf: Uint8Array<ArrayBuffer> | null = null;
  private bandsOut: number[] = new Array(48).fill(0);
  private base: PlayerState | null = null;
  private lastToken = 0;
  private pendingSeekMs: number | null = null;
  private lastPositionReport = 0;
  private ticker: ReturnType<typeof setInterval> | null = null;

  /** 播放中与否（与心跳同口径）：频谱推帧的闸——暂停冻结在最后一帧。 */
  get paused(): boolean {
    return this.base?.status !== 'playing';
  }

  constructor() {
    this.audio = new Audio();
    this.audio.preload = 'auto';
    // 挂进 DOM（隐藏）：new Audio() 是游离元素，e2e 与排障从 document 上
    // 摸不到真实音频状态；挂载不改变行为（MediaElementSource 照常接线）
    this.audio.style.display = 'none';
    document.body.appendChild(this.audio);
    this.audio.addEventListener('loadedmetadata', () => {
      this.consumePendingSeek();
      const token = this.base?.token ?? 0;
      const durationMs = Number.isFinite(this.audio.duration)
        ? Math.round(this.audio.duration * 1000)
        : 0;
      // 装载回报：Actor 从 Loading 落终态（Playing/Paused 按 pending_pause）
      void this.report({ kind: 'loaded', token, durationMs });
    });
    this.audio.addEventListener('ended', () => {
      const token = this.base?.token ?? 0;
      // 曲终回报：Actor 按 repeat/队列决定推进，快照换曲再装
      void this.report({ kind: 'ended', token });
    });
    this.audio.addEventListener('error', () => {
      // 真失败三件事一起做（审查 P1：此前只 emit error 字段——不带 status，
      // AppShell 报错通道不认；且 100ms 内被快照冲掉；且从不回报 failed，
      // 服务端 Actor 停在 playing、队列不推进，"点解不了的文件"成了假播放）
      if (!this.base) return;
      const detail = { code: 'io' as const, message: 'browser audio element error' };
      this.base = { ...this.base, status: 'error', error: detail };
      this.emit(this.base);
      void this.report({ kind: 'failed', token: this.base.token, ...detail });
    });
  }

  /** 频谱旁路装配（幂等）：失败静默降级（无频谱，不影响出声）。 */
  attachSpectrum(): boolean {
    if (this.spectrumOk) return true;
    if (this.ctx || this.spectrumAttempted) return this.spectrumOk;
    this.spectrumAttempted = true;
    try {
      const Ctor = window.AudioContext;
      const ctx = new Ctor();
      const src = ctx.createMediaElementSource(this.audio);
      const an = ctx.createAnalyser();
      an.fftSize = 2048;
      an.minDecibels = -80;
      an.maxDecibels = -20;
      an.smoothingTimeConstant = 0.55;
      src.connect(an);
      an.connect(ctx.destination);
      this.ctx = ctx;
      this.analyser = an;
      this.freqBuf = new Uint8Array(new ArrayBuffer(an.frequencyBinCount));
      this.spectrumOk = true;
      // autoplay 策略下新建的 ctx 是 suspended：音频已经过 graph 出声，
      // 挂起 = 静音冻结。装配点总在用户手势（面板打开）之后，立即恢复。
      if (ctx.state === 'suspended') void ctx.resume();
    } catch {
      this.spectrumOk = false;
    }
    return this.spectrumOk;
  }

  /** 48 对数分带读帧：公式对齐桌面 spectrum.rs（20Hz–20kHz，带内取峰）。 */
  readBands(out: number[]): void {
    if (!this.spectrumOk || !this.analyser || !this.freqBuf) return;
    this.analyser.getByteFrequencyData(this.freqBuf);
    const fft = this.analyser.fftSize;
    const half = fft / 2;
    const sr = this.ctx?.sampleRate ?? 48000;
    const BANDS = out.length;
    let binLo = 1;
    for (let b = 0; b < BANDS; b += 1) {
      const fHi = 20 * Math.pow(1000, (b + 1) / BANDS);
      let binHi = Math.ceil((fHi * fft) / sr);
      binHi = Math.max(binHi, binLo + 1);
      if (binHi > half) binHi = half;
      let peak = 0;
      for (let k = binLo; k < binHi && k < half; k += 1) {
        if (this.freqBuf[k] > peak) peak = this.freqBuf[k];
      }
      binLo = binHi;
      out[b] = peak;
    }
  }

  subscribe(cb: StateListener): () => void {
    this.listeners.add(cb);
    if (this.base) cb(this.snapshot());
    return () => this.listeners.delete(cb);
  }

  /** 服务端命令响应快照入引擎；player_seek 的参数侧真值由 args 带入。 */
  apply(s: PlayerState, seekMs?: number): void {
    const tokenChanged = s.token !== this.lastToken;
    this.lastToken = s.token;
    this.base = s;
    if (tokenChanged && s.current) {
      // 换曲装载；恢复装载（暂停态）落到记忆位置
      this.pendingSeekMs = s.status === 'paused' ? s.positionMs : (seekMs ?? null);
      if (seekMs != null) this.pendingSeekMs = seekMs;
      this.audio.src = withToken(`/audio/${encodeURIComponent(s.current.id)}`);
    } else if (seekMs != null) {
      this.pendingSeekMs = seekMs;
      this.audio.currentTime = seekMs / 1000;
    }
    this.audio.volume = Math.min(1, Math.max(0, s.volume));
    this.audio.muted = s.muted;
    if (Math.abs(this.audio.playbackRate - s.rate) > 1e-6) {
      this.audio.playbackRate = s.rate;
    }
    if (s.status === 'playing') {
      if (this.ctx?.state === 'suspended') void this.ctx.resume();
      void this.audio.play().catch(() => {
        /* autoplay 拦截：等下一次用户手势的命令再起 */
      });
    } else if (s.status === 'paused' || s.status === 'idle') {
      this.audio.pause();
    }
    this.ensureTicker();
    this.emit(this.snapshot());
    if (s.status !== 'playing' && this.ticker) {
      // 静止态只在快照变化时发一帧，不占 10Hz
      clearInterval(this.ticker);
      this.ticker = null;
    }
  }

  /** 引擎回报：走命令面归约，响应快照自动回灌。 */
  private async report(payload: Record<string, unknown>): Promise<void> {
    try {
      const s = await transport().invoke<PlayerState>('player_report', payload);
      // token 未变（同曲 loaded/position 回报）也要刷新基座：时长/位置落库口径
      if (s && typeof s === 'object' && 'token' in s) {
        const tokenChanged = (s as PlayerState).token !== this.lastToken;
        this.lastToken = (s as PlayerState).token;
        this.base = s as PlayerState;
        if (tokenChanged) this.apply(s as PlayerState);
        else this.emit(this.snapshot());
      }
    } catch {
      /* 回报失败静默：下一次事件继续 */
    }
  }

  private snapshot(): PlayerState {
    const base = this.base;
    if (!base) {
      return {
        queueLength: 0, queueVersion: 0, index: null, cursor: null,
        repeat: 'off', shuffle: false, status: 'idle', source: 'file',
        positionMs: 0, durationMs: null, volume: 1, muted: false, rate: 1,
        error: null, current: null, token: 0, updatedAtMs: Date.now(),
      };
    }
    const el = this.audio;
    const position = Number.isFinite(el.currentTime)
      ? Math.round(el.currentTime * 1000)
      : base.positionMs;
    const duration =
      Number.isFinite(el.duration) && el.duration > 0
        ? Math.round(el.duration * 1000)
        : base.durationMs;
    return {
      ...base,
      positionMs: base.status === 'playing' ? position : (this.pendingSeekMs ?? base.positionMs),
      durationMs: duration,
      updatedAtMs: Date.now(),
    };
  }

  private emit(s: PlayerState): void {
    for (const cb of this.listeners) cb(s);
  }

  private ensureTicker(): void {
    if (this.ticker) return;
    this.ticker = setInterval(() => {
      if (this.base?.status !== 'playing') return;
      this.emit(this.snapshot());
      // 位置回报节流 1s：服务端记账/持久化的数据源
      const now = Date.now();
      const pos = Math.round(this.audio.currentTime * 1000);
      if (now - this.lastPositionReport > 1000) {
        this.lastPositionReport = now;
        // 带 token：服务端按 Loaded/Ended 同口径丢弃旧标签页的迟到帧
        void this.report({ kind: 'position', token: this.base?.token ?? 0, positionMs: pos });
      }
    }, 100);
  }

  /** 装载落位：记忆位置 / seek 参数在 metadata 到位后补上。 */
  private consumePendingSeek(): void {
    if (this.pendingSeekMs != null && Number.isFinite(this.audio.duration)) {
      this.audio.currentTime = this.pendingSeekMs / 1000;
      this.pendingSeekMs = null;
    }
  }

}

/** 命令面 player_report 与 mock 一样属于 transport 内部通道（不在 client.ts 的 ipc.* 面）。 */
export const TRANSPORT_INTERNAL_COMMANDS = ['player_report'] as const;

/**
 * 登录闸预检（/auth/state 公开端点）：门开启且未持有效凭据时在 mount 之前
 * 落闸——否则首帧先渲染主界面，等第一批 401 回来才切登录卡（闪一帧）。
 * 结果写入 `__AIYA_GATE__` 供 App 初始化读取；预检失败不拦启动，401 链路
 * 仍在兜底（会话过期/撤销的运行期进闸不受影响）。
 */
export async function probeAuthGate(): Promise<boolean> {
  // 4s 超时：server 半死（TCP 收下不回）时不能把 mount 无限吊住——
  // 预检失败按"无门"处理，401 链路仍在兜底
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 4000);
  try {
    // 预检必须携带已存令牌：裸查会把"持有效会话"误判成未认证，
    // 让已登录用户刷新后被拦回登录卡（restart 实测踩中）
    const resp = await fetch('/auth/state', {
      headers: authToken ? { authorization: `Bearer ${authToken}` } : {},
      signal: ctrl.signal,
    });
    if (!resp.ok) return false;
    const state = (await resp.json()) as { required?: boolean; authenticated?: boolean };
    const gated = state.required === true && state.authenticated !== true;
    (globalThis as { __AIYA_GATE__?: boolean }).__AIYA_GATE__ = gated;
    return gated;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

export function installHttpTransport(): void {
  const params = new URLSearchParams(window.location.search);
  authToken = params.get('token') ?? readStoredToken();
  // artwork.ts 的第三前缀判定（同源 /artwork + 查询参数凭据）
  (globalThis as { __AIYA_WEB__?: { token?: string } }).__AIYA_WEB__ = { token: authToken };
  const engine = new WebPlayerEngine();

  // SSE 共享连接（多路复用，见 listen 内注释）
  let sseSourceRef: EventSource | null = null;
  const sseChannels = new Map<string, Set<(data: unknown) => void>>();
  const sseSource = (): EventSource => {
    if (sseSourceRef === null) sseSourceRef = new EventSource(withToken('/events'));
    return sseSourceRef;
  };

  const http: Transport = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      const resp = await fetch(withToken(`/ipc/${encodeURIComponent(cmd)}`), {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          ...(authToken ? { authorization: `Bearer ${authToken}` } : {}),
        },
        body: JSON.stringify(args ?? {}),
      });
      const body: unknown = await resp.json().catch(() => null);
      if (!resp.ok) {
        if (resp.status === 401) {
          // 会话过期/被撤销：App 切到登录闸（重登后整页重载）
          window.dispatchEvent(new CustomEvent('aiya:gate'));
        }
        throw body ?? { code: 'internal', message: `HTTP ${resp.status}`, detail: null, retryable: false };
      }
      // player_* 响应 = PlayerState 快照：喂引擎（seek 的参数侧真值从 args 取）
      if (cmd.startsWith('player_') && body && typeof body === 'object' && 'token' in (body as object)) {
        engine.apply(
          body as PlayerState,
          cmd === 'player_seek' ? (args?.positionMs as number | undefined) : undefined,
        );
      }
      return body as T;
    },
    listen: async <T,>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> => {
      if (event === 'player:state') {
        // 客户端自产：订阅即收当前快照，此后播放中 ~10Hz 合帧
        return engine.subscribe(cb as StateListener);
      }
      if (event === 'spectrum:frame') {
        // 频谱客户端自产：WebAudio AnalyserNode → 48 对数分带 → 帧回调。
        // 回调吃普通 number[]（Remote 源逐帧拷贝归一化，Array.isArray 校验）。
        const cbs = spectrumCbs;
        cbs.add(cb as (frame: number[]) => void);
        if (cbs.size === 1) {
          engine.attachSpectrum();
          spectrumTimer = setInterval(() => {
            if (spectrumCbs.size === 0) return;
            // 暂停/未装载不推帧：与桌面 tap 关闭同观感（分带冻结在最后一帧）
            if (engine.paused) return;
            const out = new Array<number>(48).fill(0);
            engine.readBands(out);
            for (const fn of spectrumCbs) fn(out);
          }, 33);
        }
        return () => {
          cbs.delete(cb as (frame: number[]) => void);
          if (cbs.size === 0 && spectrumTimer !== null) {
            clearInterval(spectrumTimer);
            spectrumTimer = null;
          }
        };
      }
      if (event === 'audio:mode') {
        // 输出模式是桌面专属：空订阅
        return () => {};
      }
      // SSE 多路复用：全部事件通道共享一条 EventSource（审查 P2）——浏览器
      // HTTP/1.1 每主机 6 连接上限，<audio> 的 Range 长连接还要占一条，
      // 每通道各开会在 beets 页 + 设置页同开时挤占并发槽
      let set = sseChannels.get(event);
      if (!set) {
        set = new Set();
        sseChannels.set(event, set);
        sseSource().addEventListener(event, (e) => {
          const data = (e as MessageEvent).data as string;
          let payload: unknown;
          try {
            payload = JSON.parse(data);
          } catch {
            payload = data;
          }
          for (const fn of sseChannels.get(event) ?? []) fn(payload);
        });
      }
      set.add(cb as (data: unknown) => void);
      // EventSource 自带重连；服务端连接建立即推首帧快照
      return () => {
        const cur = sseChannels.get(event);
        if (!cur) return;
        cur.delete(cb as (data: unknown) => void);
        if (cur.size === 0) {
          sseChannels.delete(event);
          if (sseChannels.size === 0 && sseSourceRef !== null) {
            sseSourceRef.close();
            sseSourceRef = null;
          }
        }
      };
    },
  };

  setTransport(http);
}
