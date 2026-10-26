//! IPC 装配层契约测试：把 UI 的 `call('name', {args})` 与 Rust 的
//! `#[tauri::command] fn name(...)` **对起来**。
//!
//! 为什么需要这一层：单元测试都是"零件级"（repo 能查、actor 能转状态），
//! 而 P0-3（7 条 playlist 命令根本没注册 → UI 调用直接 promise reject）和
//! P1-17（`app_set_ui_state` 的 Rust 参数叫 `ui_state`、UI 传 `{ state }`
//! → 永远落盘失败）都是**装配错误**：两边各自都对，接不上。
//! 这类错误只有把两份源码摆在一起比才能提前发现。
//!
//! 读的是仓库内源码（`include_str!`），所以不需要跑起 Tauri 运行时。

use std::collections::{BTreeMap, BTreeSet};

/// UI 侧的**唯一**调用源（docs/17）：`call<T>('cmd', {...})`。
/// 类型参数必须先剥掉，否则下面的解析器只认 `call('`。
///
/// 批次 4 删掉了 `ui/src/compat/api.ts`（旧信封壳）与 `ui/contracts`，
/// 所以这里不再需要合并两份源码——扫描面收敛成这一个文件。
const CLIENT_TS: &str = include_str!("../../ui/src/lib/ipc/client.ts");
const LIB_RS: &str = include_str!("../src/lib.rs");

/// UI 侧一次调用：命令名 + 入参键
#[derive(Debug, Clone, PartialEq, Eq)]
struct UiCall {
    name: String,
    keys: Vec<String>,
}

/// 扫 `call('cmd'` 并尝试抓后面的 `, { a, b }` 键列表。
///
/// 只认紧邻形态：闭引号之后是 `,` 再是 `{`。早期版本"往后找第一个 `{`"
/// 会把下一个 call 的参数误挂到当前命令上（解析器自己也得有锚点测试）。
fn parse_ui_calls(src: &str) -> Vec<UiCall> {
    let mut out = Vec::new();
    let needle = "call('";
    let mut i = 0usize;
    while let Some(pos) = src[i..].find(needle) {
        let start = i + pos + needle.len();
        let Some(end_rel) = src[start..].find('\'') else { break };
        let name = src[start..start + end_rel].to_string();
        i = start + end_rel + 1;

        let mut keys = Vec::new();
        let rest = src[i..].trim_start();
        if let Some(after_comma) = rest.strip_prefix(',') {
            let after_comma = after_comma.trim_start();
            if after_comma.starts_with('{') {
                // 花括号配对（参数对象里可能再嵌对象，如 { query: {...} }）
                let mut depth = 0i32;
                let mut obj_end = None;
                for (idx, ch) in after_comma.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                obj_end = Some(idx);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(end) = obj_end {
                    let obj = &after_comma[1..end];
                    let mut depth = 0i32;
                    let mut cur = String::new();
                    let mut parts: Vec<String> = Vec::new();
                    for ch in obj.chars() {
                        match ch {
                            '{' | '(' | '[' => {
                                depth += 1;
                                cur.push(ch);
                            }
                            '}' | ')' | ']' => {
                                depth -= 1;
                                cur.push(ch);
                            }
                            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
                            _ => cur.push(ch),
                        }
                    }
                    parts.push(cur);
                    for part in parts {
                        // 简写 `{ id }` 与 `{ id: x }` 两种形态
                        let key = part.split(':').next().unwrap_or("").trim().to_string();
                        if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            keys.push(key);
                        }
                    }
                }
            }
        }
        out.push(UiCall { name, keys });
    }
    out
}

/// 去掉 `call<Type>(` 里的类型参数，统一成 `call(`，好让同一个解析器复用。
///
/// 只处理紧邻形态 `call<`，且按 `<>` 深度找配对的 `>`（类型里可能还有
/// `Record<string, number>` 这种嵌套 + 逗号）。
fn strip_call_generics(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(pos) = rest.find("call<") {
        out.push_str(&rest[..pos]);
        out.push_str("call");
        let after = &rest[pos + "call<".len()..];
        let mut depth = 1i32;
        let mut end = after.len();
        for (idx, ch) in after.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = idx + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// 新 typed client 的全部调用。
fn all_ui_calls() -> Vec<UiCall> {
    parse_ui_calls(&strip_call_generics(CLIENT_TS))
}

/// Rust 侧注册的命令名（`generate_handler!` 宏调用里的标识符）
///
/// 用 `rfind` 而不是 `find`：注册表是 `lib.rs` **末尾**的唯一一处真实调用，而
/// `find` 会锚到第一个匹配 —— 2026-10-27 就踩过一次：往文件头注释里写了一句
/// 带方括号的宏调用示例，解析结果直接变成 0 条命令，两个测试一起红。
fn parse_registered(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(start) = src.rfind("generate_handler![") else { return out };
    let body = &src[start..];
    let Some(end) = body.find(']') else { return out };
    for item in body[..end].split(',') {
        let item = item.trim();
        let name = item.strip_prefix("generate_handler![").unwrap_or(item).trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            out.insert(name.to_string());
        }
    }
    out
}

/// Rust 侧每个 `#[tauri::command]` 函数的参数名（跳过 AppHandle/State 注入项）
fn parse_command_params(src: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let marker = "#[tauri::command]";
    let mut i = 0usize;
    while let Some(pos) = src[i..].find(marker) {
        let from = i + pos + marker.len();
        let Some(fn_pos) = src[from..].find("fn ") else { break };
        let sig_start = from + fn_pos + 3;
        let Some(paren) = src[sig_start..].find('(') else { break };
        let name = src[sig_start..sig_start + paren].trim().to_string();
        // 参数表：按括号深度切逗号（`State<'_, AppState>` 里有逗号）
        let params_src = &src[sig_start + paren..];
        let mut depth = 0i32;
        let mut end = params_src.len();
        for (idx, ch) in params_src.char_indices() {
            match ch {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        end = idx;
                        break;
                    }
                }
                _ => {}
            }
        }
        let params = &params_src[1..end];
        let mut names = Vec::new();
        let mut depth = 0i32;
        let mut cur = String::new();
        for ch in params.chars() {
            match ch {
                '<' | '(' | '[' => {
                    depth += 1;
                    cur.push(ch);
                }
                '>' | ')' | ']' => {
                    depth -= 1;
                    cur.push(ch);
                }
                ',' if depth == 0 => {
                    names.push(std::mem::take(&mut cur));
                }
                _ => cur.push(ch),
            }
        }
        names.push(cur);
        let names: Vec<String> = names
            .iter()
            .filter_map(|p| {
                let p = p.trim();
                if p.is_empty() || p.starts_with("//") {
                    return None;
                }
                // 注入参数（app/app_handle/state）由 Tauri 填，UI 不传
                let (arg, ty) = p.split_once(':')?;
                let arg = arg.trim();
                if ty.contains("AppHandle") || ty.contains("State<") || ty.contains("Window") {
                    return None;
                }
                // mut 修饰
                Some(arg.trim_start_matches("mut ").to_string())
            })
            .collect();
        out.insert(name, names);
        i = sig_start + paren;
    }
    out
}

fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn every_ui_invoked_command_is_registered() {
    let calls = all_ui_calls();
    assert!(calls.len() > 30, "解析出的 UI 调用太少，规则可能失效：{}", calls.len());
    let registered = parse_registered(LIB_RS);
    assert!(registered.len() > 30, "注册命令解析失败：{}", registered.len());

    let missing: Vec<&String> = calls
        .iter()
        .map(|c| &c.name)
        .filter(|n| !registered.contains(*n))
        .collect();
    assert!(
        missing.is_empty(),
        "UI 调用了未注册的命令（invoke 会直接 reject，UI 的 result.ok 分支走不到）：{missing:?}"
    );
}

#[test]
fn every_ui_argument_maps_to_a_rust_parameter() {
    let calls = all_ui_calls();
    let params = parse_command_params(LIB_RS);
    let mut problems: Vec<String> = Vec::new();

    for call in &calls {
        let Some(rust_params) = params.get(&call.name) else {
            // 未注册由上一个测试报；这里不重复
            continue;
        };
        for key in &call.keys {
            let snake = snake_case(key);
            let hit = rust_params.iter().any(|p| p == key || p == &snake);
            if !hit {
                problems.push(format!(
                    "{}: UI 传 `{key}`，Rust 参数是 {rust_params:?}",
                    call.name
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "参数名不匹配（Tauri 只做 camelCase→snake_case 映射，名字不同就收不到值）：\n{}",
        problems.join("\n")
    );
}

#[test]
fn parser_smoke() {
    // 解析器自身的锚点：这两段形状就是 client.ts 里的真实写法
    let calls = parse_ui_calls("call('a', { id });\ncall('b');\ncall('c', { trackIds, mode })");
    assert_eq!(
        calls,
        vec![
            UiCall { name: "a".into(), keys: vec!["id".into()] },
            UiCall { name: "b".into(), keys: vec![] },
            UiCall { name: "c".into(), keys: vec!["trackIds".into(), "mode".into()] },
        ]
    );
    let reg = parse_registered("generate_handler![\n  x,\n  y,\n]");
    assert_eq!(reg.len(), 2);
    let ps = parse_command_params(
        "#[tauri::command]\nfn f(state: State<'_, AppState>, track_id: String, app: AppHandle) -> Value {}",
    );
    assert_eq!(ps.get("f").unwrap(), &vec!["track_id".to_string()]);
}

/// UI 方法（`ui/src/stores/ui.svelte.ts` 等）会直接 await 这些 promise，
/// 只要有一个命令没注册，用户点到那个功能就是"点了没反应"。
/// 这里同时守住反向：注册了却没人用，说明契约里多了个死命令。
#[test]
fn registered_commands_have_callers() {
    let calls: BTreeSet<String> = all_ui_calls().into_iter().map(|c| c.name).collect();
    let registered = parse_registered(LIB_RS);
    let unused: Vec<&String> = registered.iter().filter(|n| !calls.contains(*n)).collect();
    // 允许内部/诊断用的命令存在，但数量必须可控（否则是契约漂移）
    assert!(
        unused.len() <= 8,
        "有 {} 条命令注册了但 UI 从不调用（契约漂移？）：{unused:?}",
        unused.len()
    );
}

/// 新 client 的**完成度**：每个注册命令都必须有 typed 入口。
///
/// 这条是 docs/17 §5 勾选表的机器版。迁移是分批做的，最容易出的错不是
/// "改错了"，而是"漏了一条没人发现"——漏掉的命令在新 UI 里等于不存在，
/// 而老壳还能跑，所以不写这条测试就永远发现不了。
///
/// 它同时挡住"新加命令只往 lib.rs 里写"：注册了但 client 没入口 → 这里红。
#[test]
fn new_client_covers_every_registered_command() {
    let called: BTreeSet<String> = parse_ui_calls(&strip_call_generics(CLIENT_TS))
        .into_iter()
        .map(|c| c.name)
        .collect();
    let registered = parse_registered(LIB_RS);
    assert!(registered.len() > 50, "注册命令解析失败：{}", registered.len());
    let missing: Vec<&String> = registered.iter().filter(|n| !called.contains(*n)).collect();
    assert!(
        missing.is_empty(),
        "新 client（ui/src/lib/ipc/client.ts）缺少这些命令的 typed 入口：{missing:?}"
    );
}
