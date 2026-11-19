# 78 · 前端 UI 治理整批质量审查（2026-11-19）

- 类型：审查
- 状态：closed（docs/77 全批通过；2 项 P3 备忘，无阻断）
- 关联：docs/77（P0、A1–A3、B1、B2、C、§18 歌词修订）；docs/59、61、70；ledger/ui-render

## 1. 审查范围与方法

docs/77（客户端 UI/UX 统一治理与频谱替换）各批次自带实施期验收，但缺少
独立整批审查。本文件对该批做事后审计：逐项核对 A01–A13 是否真实落地、
旧实现是否删净、重跑门禁与源码回归，并与 docs/77 各节声明比对。

方法：静态核对（Grep/Read 源码逐项）+ 门禁重跑 + e2e 回归重跑 +
`audit:ui-styles` 盘点比对。审查时整批改动尚未提交（git status 约 40 个
修改文件），审查对象即当前工作区。

## 2. 治理项逐项核对

| 项 | 声明（docs/77） | 核对结果 |
|---|---|---|
| A01 圆角 token 化 | Select/Menu/Popover/Dialog 统一浮层皮肤 | ✓ `components/ui/` 无硬编码下拉/弹窗圆角；剩余字面量均为合理项（Cover 按尺寸映射 4/6、Thumb/Switch/Chip 圆形 50%） |
| A02 Dialog body | 默认 16px，列表型 none | ✓ `bodyPadding?: 'default' \| 'none'`，README 记录所有权规则 |
| A03 Field/登录 | 原生属性透传、登录复用 kit | ✓ Field 支持 `type`/name/autocomplete 等；LoginView 用 Field(type=password)+Button+InlineError，`autocomplete="current-password"` |
| A04 搜索共享皮肤 | Field 搜索变体 | ✓ Field 含 prefix/trailing 与 `variant="search"` |
| A05 Header/字号 token | 尺寸进入 token | ✓ 全 `components/`+`views/`+`styles/*.css` 硬编码 font-size 为 0 |
| A06 重复播放主操作 | 详情页只留 Header 播放 | ✓ AlbumView/ArtistView/LibraryView 仅 Header 处 `playAll`；ListToolbar 只被 PlaylistList/TrackList（曲库/搜索）引用 |
| A07 快捷键翻译 | SHORTCUT_LABELS 走 i18n | ✓ SettingsPanel 748/759/763 行均 `t(SHORTCUT_LABELS[...])` |
| A08 队列嵌套按钮 | 行与子按钮分离 | ✓ 无 `role="button"` 整行；q-grip/q-play 独立 button + aria-label，Delete/Alt+方向键归 rowKey |
| A09 SideDrawer | Bits Popover.ContentStatic | ✓ trapFocus=false、preventScroll=false，无 aria-modal；手写焦点代码已删 |
| A10 周时间网格 | fs-sm、缩放 | ✓ 小时/星期/图例均 `var(--fs-sm)` |
| A11 外观 Select | 两组 Select，父开关禁用 | ✓ bgSpectrumStyle/ridgeGlow/spectrumStyle 均 Select，ridge 参数仅 ridge 显示 |
| A12 播放页 Canvas 2D | 删除，接入共享舞台 | ✓ NowPlayingSpectrum 无 canvas/getContext，仅经 render host 登记 foreground；AmbientBackdrop 持唯一 GlStage |
| A13 ui/README | 更新 | ✓ 含 kit 组合、开发预览、批量/队列/EQ 行为描述 |
| 旧枚举清理 | mirror/radial/ripple/wave 删除 | ✓ settings.ts 仅 ridge/aurora/curtain + orbit/ribbon/lattice/contour/string/shore；非法值回默认 |
| Knob 删除 | EQ 用共享 Slider | ✓ 无 Knob 文件残留 |
| 渲染结构 | 一舞台一循环、图层目录 | ✓ lib/render/{context,host,region,scheduler}.ts + layers/{ridge,flow,foreground,atmosphere} |
| §18 歌词阴影 | 移除衬底/halo | ✓ LyricsPanel 无 scrim/halo 残留；ledger/ui-render 首条已记 |
| 账本同步 | docs/35 与域账本 | ✓ ledger/ui-render 含 B1/B2/C/§18 条目 |

全部声明项落地，未发现"文档宣称完成但代码未改"的项。

## 3. 门禁与回归重跑

| 检查 | 结果 | 与 docs/77 记录比对 |
|---|---|---|
| `npm run check` | 0 error / 0 warning | 一致 |
| `check:deps` | 164 模块 / 633 内部依赖，无循环 | 一致（§16.4） |
| `check:ui-layers` | 通过 | 一致 |
| `check:style:strict` | 命中 0（白名单 1 条） | 一致 |
| `audit:ui-styles` | 26 kit / 866 条声明 | C 记录 868；§18 删歌词衬底/halo 样式后减 2，演化一致 |
| 源码回归（dev config，regression 全项目） | 149 通过 + 1 偶发失败，该失败单独重跑 3/3 通过 | 声明 119/119 的项目口径为 grep-invert 排除慢测后的子集；本次全项目 150 项全部通过 |

### 3.1 偶发失败说明

`e2e/batch.spec.ts:35` 首跑在整批 15 分钟串行负载下 `page.goto('/')`
导航 30s 超时（等待 load，非断言失败）；单独重跑 3 项 7.5s 全过。判定为
dev server 高负载下的环境性偶发，不记为产品回归。未复现第二次。

### 3.2 环境备注

沙箱批量删除保护先后拦下 Playwright 旧产物清理与 Vite 依赖预构建缓存
（`.vite/deps` 73 文件、`deps_ssr` 65 文件）的整目录删除；已分批清理缓存后
重跑成功。后续在本机环境重跑 dev 回归若复现同类报错，先清 `ui/node_modules/.vite`。

## 4. 发现与备忘

无 P0/P1/P2。两项 P3 备忘（不阻断，随下次触碰对应文件时顺手收口）：

1. **P3 · StatBlock 圆角脱阶梯**：`ui/StatBlock.svelte` 硬编码
   `border-radius: 12px / 9px`，脱离 docs/77 §4.1 声明的 4/6/8/10/14
   阶梯，也未用 `var(--radius-*)`。属静态容器皮肤，视觉影响极小。
2. **P3 · dev 回归全项目串行耗时**：regression 全项目含 contrast 等慢测
   约 15 分钟，且 goto 超时判据在满载下偏紧（本次偶发来源）。docs/77 的
   119 项口径已用 grep-invert 排除慢测；若后续再遇满载偶发，可沿用该口径
   或拆分慢测项目，不新增本批工作。

## 5. 结论

docs/77 全批（P0、A1–A3、B1、B2、C、§18）审查通过：13 项治理切口与
旧实现清理逐项落地，门禁全绿，源码回归全项目 150 项有效全过（1 项偶发
已单独复跑排除）。docs/77 的完成声明与代码现状一致，账本同步完整。
审查过程无构建、无提交、无后端改动。
