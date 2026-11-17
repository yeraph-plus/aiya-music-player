# 直播流元数据字段（LiveRecentEntry）

> 记录播放器侧直播流元数据与推流 Docker 服务的字段契约。
> 支持 **HLS 流**；元数据来自用户手填或服务端站单。
> 模型代码：`ui/src/lib/settings.ts`（类型）、
> `ui/src/lib/live.ts`（身份生成 / 合并 / 归一化）。

## 字段

| 字段 | 类型 | 必有 | 语义 |
|---|---|---|---|
| `id` | string | ✓ | **稳定身份**。与 URL 解耦：容器重启换地址，台还是那个台。UI 的 key、移除、将来与服务端同步都以它为准。新建 = `crypto.randomUUID()`；旧数据缺 id 时按 URL 派生确定性 id（`st-legacy-<hash>`），不需要写回也稳定 |
| `url` | string | ✓ | HLS 地址（m3u8）。只承担"连哪里"，不是身份 |
| `title` | string? | — | 流名。缺省回落主机名（宿主 `default_stream_title`） |
| `subtitle` | string? | — | 副标题：悬浮信息层流名下的一行小字（≤60 字） |
| `description` | string? | — | 描述：悬浮信息层说明文字，超三行截断（≤200 字） |
| `cover` | string? | — | 封面图 http(s) URL：有则铺满卡面替代图标。暂无录入入口（不做内置上传，本地文件路径未支持），为服务端站单预留 |
| `source` | `'manual'` / `'service'` | ✓ | 手工添加或服务端站单下发；用于判定字段覆盖与删除同步 |
| `addedAtMs` | number | ✓ | 入库时间（毫秒）。重开 / 补元数据不改变它——它是"何时进库"，不是"何时最后听" |

## 存储与合并

- 存放在应用设置 JSON 的 `liveRecent` 数组里（SQLite `app_settings` 键，宿主
  **schemaless 透传**——加字段零迁移，老数据缺键由读取侧归一化补齐）。
- 上限 20 条（`LIVE_RECENT_LIMIT`），**按 URL 去重**（同一地址反复开播只有一条）。
- 字段合并是**新值优先、缺位保旧**：重开同一个台不带元数据时，已填的
  标题 / 副标题 / 描述 / id / 入库时间一律保留。
- 进入直播页时刷新服务端站单：service 卡的名称、说明与封面以服务端为准，
  已删除频道的 service 卡随之移除。manual 卡保留来源和用户字段，服务端只补
  空白；即使与频道地址相同，也不因该频道被删除而移除手动流。

## 归一化

读入路径（`mergeSettings` → `normalizeLiveRecent`）逐条保证：

- 缺 `id` → 按 URL 派生确定性 id（幂等，无需写回）；
- 缺 `addedAtMs` → 0（表示"入库时间未知"，不做排序语义）；
- 缺 `source` → `'manual'`；
- url 缺失 / 非对象的条目整条丢弃。

## 服务端站单

`station_list` 提供频道摘要，新服务卡使用频道 id 与 `source: 'service'`。

| 服务端字段 | 播放器字段 | 备注 |
|---|---|---|
| 站点 id | `id` | 服务端权威；容器/端口变更不影响 |
| 播放地址（m3u8） | `url` | 可随部署变 |
| 名称 / 副标题 / 描述 | `title` / `subtitle` / `description` | service 卡使用服务端值，manual 卡仅补空白 |

## 非目标（明确不做）

- **ICY 元数据**：不解析流内 icy-name / icy-genre，元数据只来自手填与服务端。
- **封面上传**：`cover` 字段只收 http(s) URL（服务端站单可直接下发）；
  内置上传不做，"选本地图片存路径"留作将来可选。
