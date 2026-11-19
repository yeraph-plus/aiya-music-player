# 76 · Web 曲库目录与扫描状态（2026-11-18）

- 类型：计划
- 状态：closed
- 关联：docs/74、75；docs/35 近期迭代

## 范围与实现

- Web 设置页的添加目录改为 kit Dialog 与 Field，输入容器内绝对路径，
  添加后立即索引并由服务端定时追踪。空曲库入口共用组件；桌面继续调用
  Windows 目录选择器。相对路径、重复目录与服务端不存在的目录在提交时报告。
- Web 添加根目录只保存根列表，由既有服务端 action 扫描触发点执行，不再
  额外发送一次 `library_scan`。POSIX 路径比较保留大小写，Windows 路径沿用
  大小写不敏感语义，保留 `/` 和卷根路径。
- `scan:progress` 只承载扫描阶段。定时扫描保持静默，变化及失败通过既有
  `libraryRefreshed` / `libraryRefreshFailed` 通道通知，最近结果仍可从
  `refresh_state` 查询。SSE 首帧回放同样走刷新通道，并禁止重放触发重复重载。
  手动与目录 action 的进度带有效 phase，手动扫描也
  更新诊断快照。取消或失败都释放 busy，重入请求不会清除正在执行的取消信号。
- UI 忽略无阶段的进度载荷，入库阶段保持扫描中；原“上次索引异常退出 unknown”
  来自把 RefreshSnapshot 发到阶段通道，并非 worker 实际异常退出。

## 本地部署与测试音频

上一批已提交为 `5fcb7be`。本机 Compose override 将两个容器的 `/music`
从测试目录改为 `D:/FM_Full_Base:/music:ro`，实际 Mounts 的 RW 均为 false。
新曲库识别到 667 首，三个合成音频的残留索引已清除；宿主测试素材没有删除。
测试频道“新频道”仍保留 active 标记但运行失败、节目为空，已停用；其生成的
测试音频 HLS 分段、m3u 和 Liquidsoap 配置一并清除，频道信息与系统歌单保留。
运行镜像仅复制服务端 release 可执行文件、UI 产物和 beets 套件，不携带曲库，
另从 Docker 构建上下文排除 `.aiya-tmp/` 和 `test-music/`。

## 验收

- 完整 `npm run verify` 退出码 0：Svelte 0 errors / 0 warnings、transport 10/10、
  桌面 mock E2E 45/45、真实 HTTP E2E 9/9、server 单元测试 21/21 与契约 3/3；
  Rust 工作区、fmt、Clippy、75 个生成类型、其余门禁与 beets 九项冒烟通过。
- HTTP 用例验证模态框焦点、相对路径/重复/不存在路径、有效目录添加、无重复
  `library_scan` 请求、手动完成后的诊断状态；单元回归覆盖静默自动扫描、带阶段
  的手动终态、失败与取消信号、无 phase 载荷不污染状态、入库阶段 busy 与路径语义。
- `aiya-music:library-20261118` 已构建并更新 Compose，server healthy；两个容器
  `/music` 均为 FM_Full_Base 的只读挂载。实际 Edge 验证添加目录只触发一轮扫描、
  手动索引后状态正常，等待下一轮 timer 后没有额外进度帧与 unknown，无 pageerror。
- 中文/英文 960px 模态框使用既有 kit，内容和按钮无溢出。临时验证根已移除，
  设置与页面状态已恢复；曲库共 667 首、322 张专辑、216 位艺人，无离线记录和
  测试音频索引，`/app` 无测试 WAV，HLS 测试输出已清空。
- 证据：`.aiya-tmp/library-verify.log`、`library-final-docker.log`、
  `library-runtime-result.json`、`library-folder-{zh,en}-960.png`、
  `library-scan-zh-960.png`。临时物不入库。
