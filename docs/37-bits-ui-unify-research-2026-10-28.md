# 37 · UI 组件统一到 bits-ui 的可行性调研

日期：2026-10-28 · 结论先行：**维持"bits-ui 管行为、自绘 kit 管视觉"的双层架构，不做全量统一**；有两个可选的低价值迁移点（Select / Collapsible），只在出现样式痛点时再做。

## 1. 调研问题

components 目录里自定义组件居多，能否统一到 bits-ui 的设计体系？

## 2. 现状盘点（实测证据）

### 2.1 已经跑在 bits-ui 上的（9 个 wrapper）

对 `ui/src` 全量 grep `from 'bits-ui'` 的结果：

| kit 组件 | bits-ui 原语 |
| --- | --- |
| `ui/Checkbox.svelte`、`ui/CheckCell.svelte` | `Checkbox` |
| `ui/Switch.svelte` | `Switch` |
| `ui/Dialog.svelte` | `Dialog` |
| `ui/Popover.svelte` | `Popover` |
| `ui/SegmentedControl.svelte` | `RadioGroup` |
| `ui/Slider.svelte` | `Slider` |
| `ui/Menu.svelte` | `DropdownMenu` + `Portal` |
| `ui/Progress.svelte` | `Progress` |
| `ui/Tooltip.svelte`（App.svelte 亦直接用） | `Tooltip` |

这 9 个恰好是**有复杂无障碍行为**的部分（焦点管理、键盘导航、aria 状态）。也就是说行为层已经统一了。

### 2.2 完全自绘的（13 个）

- 纯视觉、零行为：`Button`、`IconButton`、`Card`、`Chip`、`Cover`、`EmptyState`、`Field`、`Icon`、`InlineError`、`Skeleton`、`StatBlock`
- 自研行为：`SideDrawer`（右缘悬浮抽屉）、`Toast`
- 直接用原生 HTML：`<select>` ×2（ImportPanel）、`<details>/<summary>` ×3（BeetsSection、ImportPanel、ScanToolsView）

### 2.3 本地 bits-ui 2.20.2 实际提供什么

`ls ui/node_modules/bits-ui/dist/bits`：accordion、alert-dialog、aspect-ratio、avatar、button、calendar、checkbox、collapsible、combobox、command、context-menu、date-field(-range)、date-picker、dialog、dropdown-menu、label、link-preview、menubar、meter、navigation-menu、pagination、pin-input、popover、progress、radio-group、range-calendar、rating-group、scroll-area、select、separator、slider、switch、tabs、time-field(-range)、toggle、toggle-group、toolbar、tooltip。

**没有 Toast，没有 Drawer/Sheet**——这两个我们正在用的自研组件在 bits-ui 里不存在对应物。

## 3. 逐项判定

| 组件 | 迁移到 bits-ui？ | 理由 |
| --- | --- | --- |
| 纯视觉 11 个 | **不迁** | bits-ui 的组件是无样式的行为层——它的设计预期就是使用者自己写视觉（shadcn 模式）。Button/Card/Chip 等没有行为可托管，换过去等于把同样的 CSS 搬个家，还多一层依赖 |
| `SideDrawer` | **不迁** | 三个手写理由都有文档且实测过（docs/25 §6）：① 几何确定（贴右缘定宽），不走 floating-ui 测量，开合零抖动——B5a 时代 Popover 壳的抖动就是测量引起的；② 遮罩**让出顶栏与播放条**（inset 挖洞），bits-ui Dialog 的 modal overlay 做不到部分穿透；③ 故意不做焦点圈禁（旁路界面）。这三条迁移后全要跟 Dialog 的 modal/focus-trap 机制搏斗 |
| `Toast` | **不迁** | bits-ui 无此组件。替代是引入 sonner（又一个依赖），而现有 Toast 已经满足"右下角、自动消退、错误常驻"三个需求 |
| `Cover` → Avatar | **不迁** | Cover 的价值在本地 artwork 的 object-URL 缓存（`lib/artwork.ts`），Avatar 只多一个 img fallback，解决的不是我们的问题 |
| `<select>` → `Select`（ImportPanel ×2） | **可选** | 收益：弹层样式与主题一致（原生 select 弹层在深色主题下是系统样式）。成本低。触发条件：真的出现样式投诉再做 |
| `<details>` → `Collapsible`（×3） | **可选** | 收益：高度动画 + 折叠样式统一。原生 details 的无障碍是浏览器原生保证的，不劣于 Collapsible。同样等痛点出现再做 |
| 新界面（Tabs/Combobox/ScrollArea…） | **直接用 bits-ui** | 新代码没有历史包袱，按既有 wrapper 模式（`ui/` 下包一层皮肤）接入即可 |

## 4. 结论

1. **"统一到 bits-ui"在字面上不成立也不划算**：行为层已统一（9/9 个复杂交互原语都在 bits-ui 上）；剩下的自绘组件要么是 bits-ui 刻意不管的视觉层，要么（SideDrawer/Toast）是 bits-ui 根本没有的能力。
2. 双层架构（bits-ui 管行为、`ui/` kit 管视觉）就是 bits-ui 官方预期的使用方式，**维持现状**。
3. 唯二值得留意的后续迁移点是 ImportPanel 的两个原生 `<select>` 和三处 `<details>`——低优先级，等真实样式痛点驱动。
4. 给后续 UI 迭代的约定：新交互一律先查 bits-ui 目录（§2.3 清单）再决定自写；凡接入，皮肤 wrapper 统一放 `ui/`，不散落在业务组件里。
