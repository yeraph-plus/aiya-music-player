# 契约草案（v0）

> 这里先放“边界形状”，不是最终字段。实现时以 Rust 类型 + 生成物为准；本文件用于评审与 golden fixtures。
> 原则：Rust 是单一来源；命令/事件/错误都有版本与序号；PCM 永不进入契约。

## 1. 版本

```json
{
  "protocolVersion": 1,
  "schemaHash": "sha256:...",
  "clientVersion": "0.1.0"
}
```

- `protocolVersion` 不兼容时拒绝连接/提示升级。
- `schemaHash` 由生成器计算；CI 校验。

## 2. Tauri 命令（UI → host）

### 播放

```json
{ "type": "play_query", "query": { "source": { "kind": "library" }, "sort": "title", "direction": "asc" }, "start": 0 }
```

```json
{ "type": "seek", "ms": 42000 }
```

```json
{ "type": "set_volume", "value": 0.72 }
```

命令返回 `PlayerSnapshot`：

```json
{
  "revision": 42,
  "generation": 7,
  "status": "playing",
  "positionMs": 42000,
  "durationMs": 245000,
  "volume": 0.72,
  "muted": false,
  "rate": 1.0,
  "repeat": "off",
  "shuffle": false,
  "queueLength": 50000,
  "index": 1234,
  "current": { "id": "tr_...", "title": "晴天", "artist": "周杰伦", "album": "叶惠美", "artworkId": "aw_...", "durationMs": 269000 },
  "error": null
}
```

### 队列分片

```json
{ "type": "queue_slice", "offset": 0, "limit": 100 }
```

```json
{ "offset": 0, "limit": 100, "total": 50000, "index": 1234, "revision": 42, "items": ["tr_a", "tr_b"] }
```

> 不返回全队列。拖动只发 `{ "type": "move", "from": 3, "to": 10 }`；后端校验后返回新 revision。

### 曲库

```json
{ "type": "library_page", "query": { "search": "晴天", "sort": "title", "direction": "asc", "limit": 200 } }
```

返回 `{ items, total, mode, nextCursor, tookMs }`；列表行不填 `audio`，单曲查询才填。

## 3. 事件（host → UI）

```json
{
  "seq": 1001,
  "revision": 42,
  "type": "player_state",
  "data": { "status": "playing", "positionMs": 42000, "...": "..." }
}
```

```json
{ "seq": 1002, "revision": 43, "type": "queue_changed", "data": { "length": 50000 } }
```

```json
{ "seq": 1003, "revision": 43, "type": "audio_status", "data": { "mode": "exclusive", "device": "Speakers", "sampleRate": 44100, "bits": 16, "bitPerfect": true, "underruns": 0 } }
```

- `seq` 单调递增；UI 丢弃旧序号。
- 高频位置/频谱可用 `Channel`，不要广播给所有窗口。
- 低频事件 4–10Hz；不要每条 timeupdate 都发全量 `Track`。

## 4. 错误

```json
{
  "code": "audio_exclusive_denied",
  "message": "设备被其他应用独占",
  "details": { "deviceId": "{0.0.0.00000000}.{...}", "hresult": "0x8889000A", "mode": "exclusive" },
  "retryable": true
}
```

UI 只按 `code` 分支；`details` 只进诊断/日志，不直接拼接给用户。

## 5. 音频 sidecar 帧

命名管道上的每帧：`u32 LE length` + UTF-8 JSON（v1）。

```json
{ "id": 1, "type": "hello", "protocol": 1, "clientVersion": "0.1.0" }
```

```json
{ "id": 2, "type": "load", "generation": 7, "path": "D:\\Music\\晴天.flac", "format": { "codec": "flac", "sampleRate": 44100, "bits": 16 }, "startMs": 0, "autoplay": true, "gainDb": null, "rate": 1.0 }
```

```json
{ "type": "position", "generation": 7, "positionMs": 42000, "playing": true }
```

```json
{ "type": "ended", "generation": 7 }
```

规则：

- `id` 用于请求/响应配对；事件无 `id`。
- `generation` 必须回带；host 丢弃过期事件。
- `path` 只存在于 host↔sidecar；UI 永远看不到。
- 控制命令有界队列不丢；`position`/`spectrum` 可丢旧帧。
- 未知字段忽略；未知 `type` 记录 warning 并忽略（向前兼容）。

## 6. 生成

```bash
# 规划中的命令；实现后固定版本
cargo run -p aiya-contract-gen -- --out ui/src/bindings.ts --schema contracts/schema.json
```

CI：

- `git diff --exit-code ui/src/bindings.ts contracts/schema.json`
- `cargo test -p aiya-core --test protocol_golden`
- `cargo clippy --workspace -- -D warnings`