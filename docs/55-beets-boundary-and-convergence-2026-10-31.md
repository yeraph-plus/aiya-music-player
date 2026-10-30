# 55 · beets 模块的职责边界与三处收敛（2026-10-31）

## 0. 一句话

宿主侧 `beets.rs` 看起来复杂，主因不是"步骤多"，是**同一件事被写了两三遍**；
这一轮把重复收敛到一份、把卸载的四道闸补上，并把"哪个插件要哪个 exe"的派生
交回工具侧。**行数几乎没降**（详见 §5，这是本节最该看的部分）。

## 1. 问题：复杂在哪（实测账目）

改动前 `src-tauri/src/beets.rs` 3106 行，切开：

| 部分 | 行数 | 占比 |
|---|---|---|
| 测试（`#[cfg(test)]` 起） | 802 | 26% |
| 注释 | 598 | 19% |
| 空行 | 144 | 5% |
| **逻辑代码** | **1562** | **50%** |

所以"长"的一半是测试与注释，两样都是资产。真正的重心在生产逻辑里：
**安装块 1005 行，占 43%**。

而安装块里，`install_via_installer`（176 行）有 110 行是"起读线程 + 行间空闲
60s + 整体 20min + 句柄收尸"——同一段骨架在 `run_bins`（91 行）里又抄了一遍，
而 `crates/aiya-beets-client/src/session.rs::spawn_line_reader` 早就是第三份
（那份还带 10MB 单行上限与 UTF-8 容错）。

**两份手写读循环都用 `BufRead::lines()`，那个 API 没有单行上限。**
pip 拼错依赖名时吐一行超长解析链，这条路径会把它整个读进内存。这不是风格问题。

## 2. 三条硬边界：为什么不能"Rust 只留协议模型 + 探测 + 转发"

用户提的目标形态基本正确，但有三步移不动：

**(a) bootstrap**（`download_to` / `verify_asset` / `fetch_asset`，95 行）。
判定"这台机器有没有 Python"的函数不能用 Python 实现——B 轨道（免配置包）
的典型用户正是没装 Python 的人。`start_install` 的第一步就是下载发布物，
那一步必须在零 Python 的机器上跑完。

**(b) `probe_python` / `py_launcher_paths`**（130 行）。同上，自指。
附带一条不能搬的：`BEETSDIR` 必须与 `AIYA_BEETS_HOME` 同值（双家隔离的地基），
这条约束不能放在被约束的那一侧。

**(c) 删除动作**。原本打算把卸载整个交给 `aiya-beets uninstall`，实测不成立：

```
# 复制解释器到临时目录，让它删自己所在的目录
DeleteFileW(sys.executable) -> 0   错误码 5（access denied）
对照：同目录新建的普通文件    -> 1（删除成功）
```

**Windows 上运行中的 exe 删不掉。** `aiya-beets uninstall` 的执行体就在被删
目录里（venv 的 `python.exe`），交给它删的结果是"删一半、留一半"，比不删更难
收拾。所以分工改成：**工具侧出计划与闸门，宿主执行删除**。

## 3. 三处改动

### 3.1 卸载：闸门归工具侧，删除归宿主

`beets-tool/src/aiya_beets/uninstall.py`：

- 新增 `--all`：目标可以是 `beets-tool` 整层（venv + bundle + bin），而不只是
  venv。宿主卸载要撤回"安装往这一层放的全部东西"，`bin/`（外置工具）与
  `bundle/`（套件包）都不在 venv 内。
- 形状闸按 `--all` 分两套：整层认 `beets-tool` 目录名或 `venv|bundle|bin` 任一；
  单 venv 仍只认 `pyvenv.cfg`。
- 新增一条闸：**目标包含当前解释器时拒绝真删**（实测依据即 §2(c)），
  并说明"由宿主或另一个目录里的脚本执行"。`--dry-run` 不受这条限制——
  宿主正是靠它拿计划。

宿主侧 `uninstall_plan()` 跑 `uninstall --all --dry-run`，把输出翻成三态：

| 返回值 | 含义 | 宿主动作 |
|---|---|---|
| `Ready { files, bytes }` | 过完闸，有账目 | 自己 `remove_dir_all` |
| `Refused(msg)` | 退出码 2 **且**有 error 帧 | **不删**，把理由报给用户 |
| `Unavailable` | 没入口 / 崩了 / 没有 done 帧 | 回落 `measure_dir` 自算 |

`Refused` 刻意不回落：闸门说"不该删"时宿主若自己删，闸门就是装饰。
反过来，退出码 2 但**没有** error 帧（工具侧自己崩了）必须当 `Unavailable`——
把崩溃误判成拒绝，用户就再也卸不掉。这两侧都写进了单测。

### 3.2 `pump_frames`：读循环三份变一份

`crates/aiya-beets-client/src/session.rs` 新增：

```rust
pub struct PumpOutcome { saw_done: bool, timed_out: bool, last_error: Option<String> }
pub fn pump_frames(stdout, idle, overall, sink) -> Result<PumpOutcome, ToolError>
```

它复用 `spawn_line_reader`，所以单行上限与非法 UTF-8 容错一并生效。只读流，
**不管 wait/kill**——句柄所有权在调用方（安装路径要把它登记进退出收尸槽）。

`install_via_installer` 与 `run_bins` 都改成用它，各自只剩"起进程 → 槽登记 →
pump → 从槽取回 wait → 判超时/退出码"。

一个必须记的细节：`bins` 在**传输期间不打进度帧**（只在开始与结束时 say），
87MB 的 ffmpeg 实测 54s。所以它的空闲阈值给 10 分钟、整体 20 分钟——
照 INSTALL 的 60s 会误杀正常下载。

### 3.3 `bins --from-plugins`：exe 清单只算一次

宿主删掉 `needed_binaries()`（用编译期内嵌表算 `exe` 并集）。现在传**插件名**：

```
<入口> bins --from-plugins replaygain,lyrics     # → ffmpeg
<入口> bins --from-plugins xtractor              # → streaming_extractor_music
<入口> bins --from-plugins lyrics                # → 空，正常收尾
```

理由：`binaries_for()` 已经在工具侧（`plugins.py`），A 轨的 `install.py`
本来就在用它。宿主再算一遍就是第二份派生——两边分家时症状是"装了插件却没下
工具"，而缺 ffmpeg 会让 beets 任何子命令都起不来。

连带删掉 `PluginTableFile.binaries` 与 `BinaryTableRow`（原挂着
`#[allow(dead_code)]`，只给单测用）。单测改走 `serde_json::Value` 读同一份
JSON，断言不变（bundle 插件的 `exe` 必须随包发）。

## 4. 实测

```
python -m aiya_beets uninstall --all --dry-run --target …/beets-tool
  → done  文件 2 / 字节 31（与落盘一致）
负向四连：
  陌生目录         → exit 2 "不像 beets-tool 安装目录"
  整层不给 --all   → exit 2 "没有 pyvenv.cfg"
  单 venv 给 pyvenv.cfg → 通过
  解释器在目录内   → exit 2，目录与 exe 都还在（没删成）

bins --from-plugins 五连：
  replaygain,lyrics → [ffmpeg]      xtractor → [essentia]
  lyrics            → [] 且 exit 0  nope → exit 2
  --from-plugins + --only → exit 2 "只能给一个"
```

测试：`beets::` 42 passed（原 40 + 2 条计划解析），`aiya-beets-client` 27 + 5
（含 3 条 `pump_frames`：帧识别 / 空闲超时只置标志 / EOF 立刻返回），
新增 `uninstall_contract.py` 8 组（已进 `verify` 与 CI）。

两条门禁各拦了一次：

- `check:boundary`（docs/33 §3.2）：client crate 的注释里不许出现工具侧词汇。
  `pump_frames` 的注释写了"pip"，报 4 处违规。改成"上游"即过——这条规则是
  对的：这一层不该知道对面是不是包管理器，知道了就会开始按包管理器做事。
- `clippy -D warnings`：`filter(..).next_back()` 与 `.last()` 互相指责
  （`filter_next` vs `double_ended_iterator_last`），最终用 `.rfind()`。
- 收尾时全仓 clippy 红在 `aiya-audio`（并行批次的 `ts.rs` 4 处 + `hls.rs` 1 处：
  `is_multiple_of` / `as_chunks` / 可省生命周期 / `== false`）。与 beets 无关，
  但不修 CI 必红，按 clippy 建议机械替换；改完 `cargo test --workspace`、
  `check:clippy`、e2e 复跑仍绿。

## 5. 收益与代价（诚实版）

基线取 `a113758`（本轮开工前的提交），统计口径见下方脚本说明。

| 指标 | 改前 | 改后 | 变化 |
|---|---|---|---|
| `beets.rs` 生产逻辑 | 1552 | 1541 | **−11** |
| `session.rs` 生产逻辑 | 491 | 542 | **+51** |
| `beets.rs` 注释 / 测试 | 589 / 737 | 626 / 822 | +37 / +85 |
| 读循环实现数 | 3 | 1 | −2 |
| exe 清单派生数 | 2 | 1 | −1 |
| 卸载闸门数 | 0 | 4 | +4 |

**行数几乎没动，合并后甚至净增 40 行。** 上一轮我估的"−280 行"是错的：那个
估算假设"删掉的部分没有替代物"，实际是"移动 + 合并"，代码量守恒，抽出来的
通用函数本身还要写 60 行，注释习惯又让每处新增配一段理由（注释 +37、测试 +85）。

口径：`git show a113758:<path>` 与工作区文件各自切 `#[cfg(test)]`，之前算生产、
之后算测试；生产里以 `//` 开头算注释，其余非空行算逻辑。§1 的 1562 是评估当轮
的读数（比基线多 10 行逻辑，来自当时还没定型的临时写法），两处差异只此一项。

所以这一轮的收益是**性质**上的，不是行数上的：三份变一份（并顺手把单行上限
找回来）、两份变一份、零道闸变四道。如果目标是"让文件变短"，这一轮做错了；
如果目标是"同一件事只有一处实现"，这一轮是对的。

## 6. 未做

- **整体薄壳化不做**。协议模型早已不在 `beets.rs` 里（`aiya-beets-client`
  1721 行），`beets.rs` 只剩薄封装；`available()`（只发现，不起进程）与
  `probe()`（起子进程 300ms）刻意分开，因为刷新管线每 30s 问一次。真正的
  复杂度来源已经处理完了。
- `install_via_installer` 里 `probe_python` + 版本窗口常量（`PYTHON_MIN` /
  `PYTHON_MAX_EXCLUSIVE`）与 `env.py` 的 `PYTHON_MIN/MAX` 仍是两份。合并需要
  决定"谁是源"（宿主内嵌 vs 工具侧下发），留给下一轮。
- **A 轨的 `run_bins` 与 `install.py` 第 5 步重复**：`install.py` 自己就在跑
  `bins.ensure(binaries_for(names))`，宿主再跑一次 `--from-plugins`。刻意保留：
  两者幂等（已 present 的文件会跳过、不联网），而 B 轨不走 `install.py`、
  没有这一步就成了"缺工具没人下"。等真机上确认 A 轨稳定后可以只留 B 轨。
- 卸载的成功路径没有在真机上点过（本轮只跑了契约测试与 `--dry-run`）。
  真机卸载要等下一次打包后验。
