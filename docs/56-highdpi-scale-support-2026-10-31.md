# 56 · 高分屏缩放支持（改动记录）

- 日期：2026-10-31
- 性质：改动记录
- 关联：docs/57（Windows 桌面 + Web 版分支方案）

## 0 · 背景

用户反馈高清屏下文字过小。调研结论（Windows/WebView2）：Tauri v2 的缩放链路（tao 自动 per-monitor DPI awareness → WebView2 `RasterizationScale` 自动跟随）本身正常，代码里也没有破坏点；「显得小」只有两类真因——

1. **设计字号偏小**（body 13px + 4K 大窗口视觉密度低）：应用自己的旋钮解决；
2. **环境性失效**（RDP 不透传 DPI / 旧 WebView2 Runtime / 跨屏未刷新，特征是 dpr=1.0）：诊断识别 + 用户侧兜底。

因此落地方案 = tokens px→rem + 「界面缩放」档位（正解）+ 「禁用系统缩放」开关（环境逃生门）+ DPI 诊断日志（分诊）。

## 1 · 设计模型：两个正交的旋钮

| 旋钮 | 改什么 | 默认 | 生效方式 |
|---|---|---|---|
| 界面缩放（外观组） | `html` 内联 font-size，rem 尺寸轴整体跟随——**用户倍率** | 100% | 前端即时（look 链路） |
| 禁用系统缩放（系统组） | webview zoom 补偿为 1/scale——**环境倍率**强制 1 | 关（跟随系统） | Rust 即时（zoom 链路） |

两者**相乘**：150% 屏上「禁用系统缩放 + 界面缩放 125%」= 有效 125%。互不感知、互不依赖。

关键决策：

- **rem 而非 CSS zoom / transform**：rem 是文字重排方案，不糊、canvas 天然不受影响（渲染层已按 dpr 处理）；CSS zoom 跨引擎支持不齐，transform 是位图放大，均否决。
- **zoom 补偿而非 `--force-device-scale-factor`**：后者要建窗前注入（改了重启），且 `additionalBrowserArgs` 会整体替换 wry 默认参数；zoom 是运行时操作，即时、可逆。
- **zoom 由 `apply_scale_policy` 独占管理**：将来若开 `zoomHotkeysEnabled` 让用户手调缩放，要先改造成记忆用户值，否则每次跨屏被重置。
- **不替用户做决定**：两旋钮默认值 = 现状（100% / 跟随系统）。

## 2 · 改动清单

| 文件 | 内容 |
|---|---|
| `ui/src/styles/tokens.css` | 尺寸轴 px→rem（§3） |
| `ui/src/lib/settings.ts` | `uiScale: UiScale`（90/100/110/125/150 白名单 + `normalizeUiScale` 收口 + `UI_SCALE_OPTIONS`）；`disableSystemScale: boolean` |
| `ui/src/lib/ipc/schema.ts` | 两键的 zod 项（uiScale 只验数值，档位白名单在 mergeSettings） |
| `ui/src/lib/settings-migrate.ts` | `merged.uiScale = normalizeUiScale(...)` 白名单收口（手改配置出现 87.5 → 落 100） |
| `ui/src/lib/look.ts` | `Look` 增加 `uiScale`（首帧镜像随主题走）；`applyLook` 写 `html` 内联 font-size；`readRowHeight` 探针化（§4） |
| `ui/src/App.svelte` | look effect 携带 `uiScale`（档位变化重读行高，虚拟列表重新对齐） |
| `ui/src/components/SettingsPanel.svelte` | 外观组「界面缩放」分段控件；系统组首位「禁用系统缩放」开关 |
| `ui/src/lib/dpi-diagnostics.ts`（新） | `[dpi]` 诊断日志（§6） |
| `ui/src/main.ts` | 挂载前 `applyLook(readLook())` 后启动诊断 |
| `ui/src/env.d.ts` | `__AIYA_LOOK__` 类型补 `uiScale?` |
| `src-tauri/src/lib.rs` | `apply_scale_policy` + 三处接线（§5）。设置真值是 `app_settings` KV passthrough，**零 Rust 结构改动** |

## 3 · tokens rem 迁移细则

- 迁移范围：`--space-*`、`--radius-xs..xl`、`--control-h`、`--row-h`、`--titlebar-h`、`--sidebar-w`、`--player-h`、`--fs-*`。**100% 档与旧 px 值逐一相等**，历史视觉零变化（注释保留原 px 对照）。
- 刻意留 px：阴影/模糊（深度线索不随缩放）、`--radius-pill`（"够大"语义）、组件内联 1px 发丝线与位移。
- Tailwind 侧无需动：`text-*`/spacing 本来就是 rem，天然跟随。
- **教训（探针化）**：`getComputedStyle().getPropertyValue()` 对自定义属性**不换算单位**——`--row-h: 3.25rem` 只会原样返回 `"3.25rem"`，原 `parseFloat` 实现会拿到 3.25，虚拟列表静默错位。现改为探针元素实测 px；**不 round**（行高与虚拟列表步长必须同源，round 会逐行积累 0.x px 漂移）。

## 4 · 界面缩放链路（首帧无闪变）

```
设置真值(SQLite app_settings) ──bootstrap──> ui.settings.uiScale
        │                                        │ App look $effect
        │                                        ├─ applyLook() → html 内联 font-size
        │                                        └─ writeLook() → localStorage 镜像
镜像 ──main.ts mount 前──> applyLook(readLook())   （真值到前先按上次档位画）
```

与主题同一班车（`look.ts` 的既有模式），main.ts 零新增时序。档位外的值（`normalizeUiScale`）一律落 100。

## 5 · 禁用系统缩放（Rust）

`apply_scale_policy(app, scale_hint)`：开启时 `set_zoom(1/scale)` → 内容 100% 物理密度（dpr≈1，重排渲染不模糊）；关闭时 `set_zoom(1.0)`。三条触发路径：

1. **启动**：setup 内（窗口已建、设置缓存已灌后）对齐一次；
2. **开关变化**：`app_set_settings` 的 diff 模式（同 `output`/`normalizeVolume`）→ 立即应用；
3. **跨屏/系统缩放变更**：`on_window_event` 的 `ScaleFactorChanged`，用**事件携带的新 scale** 重算（此刻 `win.scale_factor()` 可能还是旧屏值）。

## 6 · DPI 诊断日志

`[dpi] dpr=… viewport=…(css) scale=… zoom≈… inner=…(physical) logical=…`，启动一次 + resize（防抖 400ms）一次，info 级。分诊口诀：

- dpr ≈ 系统缩放 → 缩放正常，是字号/档位问题；
- dpr=1.0 而系统 150% → 缩放失效（RDP / 旧 Runtime / 跨屏），查环境；
- `zoom≈1/scale` → 「禁用系统缩放」开启中（150% 屏上 ≈0.67），别误诊；
- `logical` 与 tauri.conf.json 的 1280×800 对不上 → 物理/逻辑混用回归哨兵。

## 7 · 验证

- `cargo check`（宿主）0 错误；`svelte-check` 0 错误；`vite build` 通过；e2e 44/44（含虚拟列表、设置页）。
- 真机清单（150% 屏）：开「禁用系统缩放」→ 立即整体变小、`[dpi]` dpr=1.0/zoom≈1.00；拖去 100% 副屏再拖回 → zoom 跟随新 scale；关开关 → 立即恢复；界面缩放切 125% → 行高/文字/间距同倍变化、列表不错位。

## 8 · 遗留项（后续批）

- lucide 图标 `size={n}` 是 px 属性（约 100 处）：非 100% 档下相对文字略小 → px→em 映射扫描。
- 组件 `<style>` 里散落的布局 px（边框/位移类为刻意保留，其余按需收编）。

## 附 · 关键依据（精简）

- tao 运行时 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`；WebView2 `ShouldDetectMonitorScaleChanges` 默认 true，`RasterizationScale` 自动跟随（源码级 + [spec](https://github.com/MicrosoftEdge/WebView2Feedback/blob/main/specs/RasterizationScale.md)）。
- webview 内 1 CSS px = 1 逻辑 px；配置窗口全是逻辑像素（schema 确认）。
- `Webview::set_zoom` → WebView2 `SetZoomFactor`（wry 源码确认）；`additionalBrowserArgs` 整体替换 wry 默认参数。
