# ADR-0004：Rust 契约单一来源，生成 TypeScript

- 状态：Accepted
- 日期：2026-10-23

## 背景

现版 `packages/contracts` 是 TS 单一来源，主/渲染共用，配合 Zod 校验，质量很高。但 Tauri 重写后后端是 Rust；如果继续手写 TS 类型，就会出现“Rust struct / TS interface / JSON 文档”三份真相。

## 决策

领域命令、事件、错误、DTO 全部定义在 Rust（`aiya-player` / `aiya-app`），用 `specta` + `tauri-specta` 生成 `ui/src/bindings.ts`。CI 校验生成物与提交一致，并把 schema hash 写入 `contracts/schema.hash`。

## 后果

正面：

- 编译期发现前后端字段漂移。
- 命令/事件/错误只有一份定义。
- 生成物可 review，协议变更可见。

负面：

- Rust 类型设计要兼顾 serde/specta（tagged enum、可选字段）。
- 生成器/版本升级会带来 diff；需要固定版本。
- 复杂泛型/递归类型可能不支持，需要扁平化 DTO。

## 约束

- 不手改 `bindings.ts`；生成命令写入 `package.json`/CI。
- 协议变更必须更新 `PROTOCOL_VERSION` 或提供向后兼容字段。
- 事件/命令用 tagged enum，避免 `any`。
- sidecar 协议独立生成/定义，不复用 Tauri 类型，保持可单独测试。
- **v1 UI 兼容例外**：为了原样搬旧 UI，先复制旧 `packages/contracts` 的 `AiyaApi` 形状，Rust 按此返回 DTO；`bindings.ts` 并行生成，等 UI 重写时切换。这是临时适配，不是第二份长期契约。

## 否决的替代方案

1. **TS 继续做单一来源**：Rust 侧要手写镜像，必然漂移。
2. **OpenAPI/JSON Schema 手写**：多一层维护，收益低。
3. **只靠运行时 Zod**：能在边界发现错误，但开发期无类型提示。
