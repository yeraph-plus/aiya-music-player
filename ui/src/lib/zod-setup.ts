/**
 * 必须在**任何模块构造 zod schema 之前**求值。
 *
 * zod v4 判定"能不能用 JIT"是 `Function('')` 探测 + 缓存（`jitless` 为真就跳过），
 * 而探测在**构造** schema 时就可能发生。所以写在 `main.ts` 的顶层语句里已经太晚——
 * 那时 `App.svelte` 整条 import 链（含 settings 的 schema）已经求值完。
 *
 * 生产 CSP 是 `script-src 'self'`（禁 eval），探测必然失败：失败后 zod 自动走解释器
 * 路径，功能正常，但每次启动都在 console 留一条 CSP 违规红字，把以后真被拦的东西
 * 盖掉。实测证据：生产构建下 `securitypolicyviolation` 只有这一条。
 *
 * 代价是 schema 解析走解释器；设置只在启动与保存各解析一次，可忽略。
 */
import { z } from 'zod';

z.config({ jitless: true });
