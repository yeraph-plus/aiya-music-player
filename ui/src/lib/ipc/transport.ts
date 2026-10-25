/**
 * 宿主通信的**唯一换气口**（命令 + 事件）。
 *
 * 为什么要有这一层：批次 4 要恢复浏览器预览（`npm run dev:ui` 无 Rust 可跑），
 * 而 `client.ts` / `events.ts` 里如果各自写一遍
 * `if (thereIsNoTauri) useMock()`，Tauri 生产包里就会留下两条分支和一个
 * 永远走不到的死代码块。改成注入：默认实现是 Tauri，预览时由
 * `lib/ipc/mock` 在 `main.ts` 里**动态 import** 后整体替换 —— 生产构建里
 * mock 是独立 chunk，不会被加载。
 *
 * 一次替换同时覆盖命令与事件，所以订阅侧（10Hz 的 `player:state`、
 * 30fps 的 `spectrum:frame`）在预览下也照常工作，不需要各自判断环境。
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Transport = {
  /** 与 Tauri `invoke` 同签名：失败应当 reject（`client.ts` 再归一化成 ApiError） */
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  /** 订阅事件，resolve 为同步 unsubscribe（与 `events.ts` 的 sub 契约一致） */
  listen: <T>(event: string, cb: (payload: T) => void) => Promise<UnlistenFn>;
};

const tauriTransport: Transport = {
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args),
  listen: <T>(event: string, cb: (payload: T) => void) =>
    listen<T>(event, (e) => cb(e.payload)),
};

let current: Transport = tauriTransport;

/** 取当前实现。Hot path 每次都调，别在这里做分支以外的事。 */
export function transport(): Transport {
  return current;
}

/** 传 `null` 恢复 Tauri 实现（测试清理用） */
export function setTransport(next: Transport | null): void {
  current = next ?? tauriTransport;
}

/** 当前是否跑在真宿主里（预览模式 = false）。诊断/开发提示用，业务逻辑不该依赖它。 */
export function isTauriRuntime(): boolean {
  return current === tauriTransport;
}
