# 74 · 双宿主接缝修复（2026-11-17）

- 类型：审计
- 状态：closed（修复与验证完成）
- 关联：docs/65、66、68；AGENTS §3 宿主范围

## 范围与决定

本轮审查覆盖同仓 main 的 Tauri 与 HTTP/Docker 宿主。共享数据、DTO、PlayerActor
和 BeetsHost 保持现有依赖方向；原生音频不进入 server 编译图。

HTTP 播放使用每个页面独立的串行命令通道：player 命令、live_open、引擎回报
按提交顺序执行及应用响应，单次请求超时 15 秒，失败不阻塞后续命令。位置回报
最多一个在途，避免慢网络积压。该机制保证本页面的命令顺序，不增加多客户端
远程控制或主播放端选举；那部分仍属 docs/69 的独立范围。

Web 收听复用 transient_stream 和服务端 Actor。站内 `/hls/{dir}/live.m3u8`
直连，外部 http(s) 经 `/stream`；支持 MSE 的浏览器使用 hls.js，其余回落原生 HLS。
私有频道清单把 token 传递给子清单、密钥和分段，暂停释放连接，恢复重新装载。
Tauri 不加载 HTTP 音频 chunk。

## 修复面

- player_report 在同一 Actor 锁内校验 token、归约并提取记账数据，旧回报没有
  错误或历史副作用；位置缓存也带 token，避免新曲未上报时沿用旧曲秒数。
- SSE 通道取消最后订阅时移除 EventSource 原生 handler。
- 流代理关闭自动跳转，逐跳验证 scheme 与解析地址，并将已验证地址固定给本次
  连接；清单相对路径基于最终 URL，URI 属性也进入代理。
- server 扫描使用独立 SQLite WAL 连接，手动和定时扫描复用执行函数；暂停根按
  路径组件及宿主大小写语义比较。根计数/移除前缀与共享 path_key 对齐。
- 节目表保留 HH:MM，跨日推进星期，输入拒绝非法时间与空星期集合；compose
  两服务统一 TZ。keeper 只在进程存活到下一轮后清零崩溃计数。
- Compose 使用 `entrypoint: ["sh", "/keeper.sh"]` 覆盖官方镜像入口，确保
  keeper 由 shell 执行。
- test:all 纳入 transport 回归，verify 与 Windows CI 纳入 HTTP E2E；Linux CI
  纳入 keeper 秒崩上限及稳定恢复测试。

## 验证

- `npm run verify` 全部通过：Rust 工作区测试、UI 类型检查、依赖/边界/协议门禁、
  UI 构建、fmt、clippy（`-D warnings`）、72 个生成类型校验、注释门禁及
  beets 九项冒烟。svelte-check 为 0 error / 0 warning。
- HTTP transport 回归 5/5、桌面 mock E2E 44/44、真实 HTTP 宿主 E2E 7/7。
  私有频道在系统 Edge 中实际推进播放，验证了分段鉴权、断开释放及重新连接。
- keeper 在隔离 Linux 容器中以快速退出的替身进程验证：达到 3 次启动后停止
  重试，关闭并重新启用后可稳定启动并清零计数。两次运行均通过。
- `docker compose config --quiet`、`git diff --check` 通过；测试 server 与临时
  容器已退出，现有服务未变更。

Linux 路径大小写测试已纳入条件编译测试，交由 Linux CI 执行；原生 HLS 回退
与真实 WASAPI 出声本轮未实测。

## Docker 构建与真实出流补验

开启 TUN 后官方镜像成功拉取。`docker build --progress=plain -t
aiya-music:review-20261117 .` 全部通过，覆盖前端构建、Linux release 编译、
beets Linux 套件打包及包内自检。产物大小为 428,038,344 字节，镜像 ID 为
`sha256:955ad16640a21ede7ab0deacec7fe2a3b20ad220f6bead90bc0f10b8d10aba12`。

真实镜像 `savonet/liquidsoap:v2.4.6` 的 Entrypoint 为 `/usr/bin/liquidsoap`，
原 Compose 的 command 覆盖会变成 `liquidsoap sh /keeper.sh`，将 shell 脚本
当作 Liquidsoap 配置解析，实测报 `$` 语法错误。改为 entrypoint 覆盖后双容器
正常启动。Liquidsoap 镜像 digest 为
`sha256:63966c36c5ea208ba56fac787e955502b348eb00c5c32cdd134ee5ec1d20de53`。

在独立 Compose 项目、临时数据卷与只读测试音频挂载下完成：

- server 健康检查为 healthy，5 首测试音频成功扫描。
- 真实 Liquidsoap 对服务端生成的 station.liq 执行 `--check` 成功；覆盖分钟
  时段、周日跨日到周一以及全日节目表。独立时间谓词检查同样通过。
- keeper 在官方镜像内的重试上限及稳定恢复回归通过。
- 频道实际生成主清单、子清单、MP3 分段及 nowPlaying 回灌；匿名访问私有
  频道返回 401，带凭据逐层获取清单和分段成功。
- 系统 Edge 访问容器内构建的 UI，通过频道卡片收听真实 Liquidsoap HLS，
  实测 audio 未暂停、currentTime 推进且没有播放错误或页面异常。

临时双容器、网络和两个数据卷已清理，构建镜像保留供复用。现有运行服务未变更。


## 整批复核（2026-11-18）

与 docs/75 的最终页面及节目模型一并复核。HTTP 串行命令、旧 token 副作用隔离、
两宿主队列结构持久化、逐跳校验与 DNS 固定、独立扫描连接的实现与测试保持有效。
上文测试数量与镜像均为当时阶段记录，最终证据见 docs/75 的整批收尾。

本次补修 keeper 死亡 .pid 残留、跨频道 WebSocket 曲名事件、当前节目切歌出口，
以及引擎就绪/失败清理和 InProcess 守护停止接缝；细节、回归与实际引擎验证统一
记录在 docs/75。Tauri 原生音频不进入服务端编译图，浏览器音频不进入桌面运行分支。

原生 HLS 回退与真实 WASAPI 出声仍未覆盖，Linux 路径条件测试由 CI 承接。
