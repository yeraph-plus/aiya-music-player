# 代码结构评估（2026-10-27）

问题：这套代码的前后端分隔与组织形态，跟一般项目差得远，好还是不好？

**结论先给**：

1. **前后端分隔是好的，而且好得有点罕见**——它不是靠约定维持，是靠三层机器门禁维持的
   （`gen:types:check`、`check:boundary`、`ipc_contract`）。
2. **组织形态是"薄宿主 + 厚 crate + 契约生成 + store 聚合"**。你觉得陌生，是因为它跟
   最常见的"组件里直接 invoke、后端一个大文件"正好相反，**不是因为它乱**。
3. **有一处真实的结构债**：`src-tauri/src/lib.rs` 3127 行 / 68 条命令，该拆。
4. **有两处"看起来怪但刻意"的**：参数走 `Json` 手解析、mock 与真后端并行。都是拿代价
   换确定性，不是偷懒。
5. **有一处该警惕**：16 个 store 之间 21 处互相 import，今天清楚，再长会变成隐式全局。

---

## 1. 这套结构是什么

```
┌─────────────────────── ui/ (Svelte 5) ───────────────────────┐
│  views/      10 个路由页        1435 行                       │
│  components/ 18 个部件          4344 行   ← 0 处引用 IPC      │
│  stores/     16 个 $state 聚合  2824 行   ← 只有 3 个碰 IPC   │
│  lib/        30 个纯函数         3066 行   ← 2 个碰 IPC       │
│  lib/ipc/    transport → client/events → generated(57 类型)   │
└──────────────────────────┬───────────────────────────────────┘
                           │ invoke / listen（唯一换气口）
┌──────────────────────────┴───────────────────────────────────┐
│  src-tauri/  宿主：AppState + 68 条命令 + 事件泵    7920 行    │
│    beets.rs 发现/安装  ·  refresh.rs 刷新管线  ·  pump.rs 事件 │
│    sidecar.rs 子进程  ·  smtc.rs 系统媒体  ·  dto/ 9 个域     │
└──────────────────────────┬───────────────────────────────────┘
                           │ 只依赖（反向不存在，编译期保证）
┌──────────────────────────┴───────────────────────────────────┐
│  crates/                                                      │
│    aiya-data    5540 行  SQLite + 扫描 + 播放列表 + beets 对账 │
│    aiya-audio   5378 行  Symphonia 解码 + WASAPI + DSP/EQ     │
│    aiya-player  1450 行  PlayerActor 状态机                   │
│    aiya-beets-client 996 行  只懂协议，不懂环境（门禁守）      │
│    aiya-audio-host   286 行  独立进程，命名管道 JSON           │
└───────────────────────────────────────────────────────────────┘
```

`aiya-audio-host` 是独立**进程**（不是模块）——音频崩溃不带走 UI，代价是 PCM 不过管道、
只过控制指令。

## 2. 五条可测判据（都是实测，不是印象）

| 判据 | 一般项目常见做法 | 本项目实测 | 判断 |
|---|---|---|---|
| **依赖方向单向？** | crate 互相引用，宿主逻辑漏进库 | `crates/*` 全都不依赖 `tauri`；`src-tauri` 单向依赖 5 个 crate | ✅ 编译期保证 |
| **跨边界形状有单一来源？** | 前后端各写一份 interface | 返回值由 `ts-rs` 生成 **57 个 .ts**，`gen:types:check` 进 `verify`，漂移即红 | ✅ 机器守 |
| **有没有越级调用？** | 组件里 `invoke(...)` | **components 与 views：0 处**引用 `ipc/client`；只有 3 个 store + 2 个 lib 文件碰 | ✅ 分层真实存在 |
| **单文件是否失控？** | 到处都是 2000 行文件 | 全部 17 个 rs 文件里只有 1 个超 1500 行是命令层（`lib.rs` 3127）；最大业务文件是 `engine.rs` 2368（引擎本体，合理） | ⚠️ 一处该拆 |
| **结构约束是否机器化？** | 写在 wiki 上，没人执行 | `check:boundary` 12 条规则 + `ipc_contract` 5 条装配测试 + `proto_mirror` 5 条（对着 schema 验镜像） | ✅ 罕见 |

**第 3 条值得展开**：`components` 与 `views` 里 **0 处**直接引用 IPC 客户端。
这意味着组件只认 props 和 store，换后端不用动任何组件。绝大多数项目做不到这一条
——它们的组件里总有几处"顺手 invoke 一下"。这不是自觉，是因为 `client.ts` 不在
`components` 的 import 白名单里形成习惯后，代码评审自然就挡了。

**第 5 条值得展开**：`tools/check-boundary.mjs` 不是 `grep 禁词`，它写了规则取舍：

```
· python 例外 pythonVersion —— 那是协议线上字段名，改名就不是这个协议了
· beets 不作禁用词 —— 包名与协议字段必然带它；禁的是内部概念（插件名、库文件名）
· 大小写敏感 + 词边界 —— 否则 Stdio::piped 会命中小写 pip，门禁变噪音后一定被关掉
```

"门禁变噪音后一定会被关掉"这句话说明作者知道门禁的真正失效模式。

## 3. 你觉得陌生的三个地方，其实各有理由

### 3.1 命令全堆在一个文件（`lib.rs` 68 条）

Tauri 的 `generate_handler![...]` 要求一个列表，于是几乎所有项目都把所有命令写进
`lib.rs`。这是**框架惯例**，不是本项目的发明。问题只是它没拆到 3127 行还拆得动。

### 3.2 参数收 `Json` 再手动解析（返回值却是强类型）

不对称，但理由写在 `client.ts` 头注释里：参数反序列化失败发生在**命令体之前**，
用强类型参数会让"参数不合法"变成 Tauri 自己的一句 reject，绕开 `ApiError` 的分类，
前端拿不到 `code`。所以：

- 返回值：`ts-rs` 生成（强类型，`gen:types:check` 守）
- 参数：手写结构 + 后端 `Json` 手解析（**只有名字级校验**，`ipc_contract` 扫
  `every_ui_argument_maps_to_a_rust_parameter`——能抓"名字写错"，抓不到"类型写错"）

代价是真的：前端传 `{ limit: "10" }` 不会编译报错，要等运行时。

### 3.3 mock 与真后端并行（`mock/backend.ts` 1247 行）

`transport.ts` 是**唯一换气口**，浏览器预览时整层替换 mock。好处是生产包里 mock 是
独立 chunk、不会加载，也不需要每个调用点写 `if (isTauri)`。代价是**两套行为要人肉对齐**
——`mock 里 beets.found 恒 true` 就是这样漏掉的（见 docs/35 P2-11）。

## 4. 真实成本（这套结构贵在哪）

| 要改的地方 | 这套结构要你做 |
|---|---|
| 加一条命令 | ① Rust 写命令并注册 ② `dto/` 加类型（带 ts-rs）③ `npm run gen:types` ④ `client.ts` 加方法 ⑤ mock 加分支 ⑥ 可能要改 `ipc_contract` 期望 |
| 改一个字段名 | 生成物会红，`gen:types:check` 会红，前端所有用到处会红——**这是优点，也是摩擦** |
| 新人读代码 | 必须先懂 `transport → client → store → component` 四层，否则会在组件里找不到网络调用 |

**换来的**：契约漂移变成编译/门禁错误，而不是线上白屏。这个项目 336 个 Rust 测试 +
26 条 e2e 里，有一半是"两边能不能接上"的装配测试，不是业务测试。

## 5. 三个真问题（按性价比排序）

### 5.1 `lib.rs` 该按领域拆（唯一的结构债）

现状：`AppState`（约 120 行）+ 68 条命令 + 内联业务 + 测试，全在一个文件。

建议形态（**行为零变化**，纯移动）：

按实测前缀分布（68 条，全部统计自 `lib.rs`）：

```
player 20 · library 13 · playlist 12 · beets 11 · app 7 · history 2 · spectrum 1 · refresh 1 · lyrics 1
```

建议形态（**行为零变化**，纯移动）：

```
src-tauri/src/
  state.rs        AppState + 构造（现在 lib.rs:206-340）
  setting.rs      setting_get/set + DEFAULT_SETTINGS（现在内联，refresh 也要用）
  commands/
    app.rs         8 条：app 7 + lyrics 1
    library.rs    15 条：library 13 + history 2
    player.rs     21 条：player 20 + spectrum 1
    playlist.rs   12 条：playlist 12
    beets.rs      12 条：beets 11 + refresh 1
  lib.rs           只留 builder + generate_handler! + 事件接线    (~300)
```

拆完 `lib.rs` 的三千行会散成五个 400–700 行的域文件，`AppState` 单独一个文件。
**这一拆不需要改任何命令体**，`ipc_contract` 与 336 个测试就是它的验收。

不做会怎样：每条新命令都在给同一个文件加长度，且 `AppState` 的字段说明（写得很好的
"为什么"注释）会被埋进 3000 行里。**但这不是收尾项，不进 docs/35**——它不影响发布，
只影响"下一个人还愿不愿意改"。

### 5.2 参数契约只有名字级校验

`ipc_contract` 能抓"前端传了 `limit`，Rust 参数叫 `limit_take`"，抓不到"传了字符串、
Rust 期望数字"。补法二选一：

- **便宜**：给参数也出 ts-rs 类型（让参数反序列化在命令体内做，仍能包成 `ApiError`）
- **贵**：引入 `specta` 一类自动生成 bindings 的方案（要重做整个契约层，不值）

### 5.3 store 之间的隐式依赖

16 个 store，21 处互相 import：`notify`(6)、`ui`(4)、`library`(4)、`list`(3)。
今天是清晰的（依赖有方向、没有环），但"谁都能调 notify"这类设计再长两年会变成
隐式全局。可选约束：让 `notify` / `ui` 只被 `views` 层调用，store 里改成返回
错误让调用方决定——**但这条收益不确定，别现在做**。

## 6. 不要动的地方

- **不要合并 crates**。`aiya-data` / `aiya-audio` / `aiya-player` 分开是为了"能单独编译
  测试"，合并后 336 个测试跑得慢、且音频依赖（wasapi）会污染数据层。
- **不要让 `aiya-beets-client` 认识 Tauri 或环境**。这是全项目最脆的一条边界
  （两个生态、两种语言），已有门禁，别为了省一层适配去破它。
- **不要为了"看起来整齐"把 `dto/` 并进命令**。它是跨边界形状的唯一居住地。
- **不要去掉 mock**。它是 `npm run dev:ui` 能跑的唯一原因，也是 e2e 的半边地基。

## 7. 一句话

**这套结构比一般项目更严格，代价是改一个字段要多跳几个地方；它严格的部分都有机器
在守，所以不会随时间腐化；唯一在腐化的是 `lib.rs` 的长度，而那个文件恰好是最容易
拆、风险最低的地方。**
