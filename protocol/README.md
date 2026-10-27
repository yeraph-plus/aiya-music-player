# protocol/ — beets 组件协议的**镜像**

```
beets-tool/protocol/v1.schema.json    ← 真源（随 beets 组件发版，在工具仓里）
protocol/aiya-beets-v1.json           ← 本目录：字节副本
protocol/aiya-beets-v1.meta.json      ← 来源、版本、sha256
```

## 为什么是镜像而不是"共享一份"

两侧是**独立发布单元**（docs/33）：组件有 `beets-v*` tag，播放器有自己的版本号。
如果只有一份文件，任何一侧改了它另一侧就"自动跟随"——协议漂移会变成静默事件。

镜像 + 手动同步 + sha256 的代价是每次改协议多一步 `npm run proto:sync`，
换来的是**协议变化永远出现在 diff 里**。

## 怎么用

```bash
npm run proto:check   # CI：字节一致 + meta 自洽（进 test:all，不需要 Python）
npm run proto:sync    # 改完真源后同步镜像（会重算 sha256/版本）
```

三个地方在验同一件事的不同侧面：

| 谁 | 验什么 |
|---|---|
| `beets-tool/tests/protocol_contract.py` | 工具**真吐出来的**每一行都合 schema（服务端视角） |
| `crates/aiya-beets-client/tests/proto_mirror.rs` | `proto.rs` 的 serde 字段集合 == schema 的 `required`（客户端视角） |
| `src-tauri/tests/dto_wire.rs` | 能力段 DTO 的字段名 == schema（中间层视角） |
| `npm run proto:check` | 镜像没被单方面改过（分发视角） |

## 版本语义

`protocolVersion` 目前是 **1**，冻结于 2026-10-27。

- 加可选字段、加新命令、加新事件 → **不升版本**
- 改名、删字段、改类型 → **升版本**，且两侧要同时改
- 版本不符不协商：宿主探测到不符就 `found: false` + `E_PROTOCOL_MISMATCH`，
  文案里说清升哪一边（不做"半好半坏"的降级）

工具在 `status` 里另报一个 `schemaHash`（本 schema 的 sha256）。那是**运行时**凭据：
将来播放器与组件分别升级时，靠它一眼判断"对面是不是同一版契约"。
