//! HTTP 命令面契约守卫：server 路由表（`ipc.rs` 的 ROUTED）与 UI 的唯一调用源
//! `client.ts` 对起来。与桌面壳的 `tests/ipc_contract.rs` 同思路——装配错误
//! （路由名拼错、UI 用了未路由命令）只有把两份源码摆在一起比才能提前发现。

use std::collections::BTreeSet;

const CLIENT_TS: &str = include_str!("../../../ui/src/lib/ipc/client.ts");
const IPC_RS: &str = include_str!("../src/ipc.rs");

/// 扫 `call('cmd'` 与 `call<T>('cmd'`（类型参数先剥掉再认引号，与
/// src-tauri/tests/ipc_contract.rs 同一口径）。
fn parse_client_calls(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let needle = "call";
    let mut i = 0usize;
    while let Some(pos) = src[i..].find(needle) {
        let mut j = i + pos + needle.len();
        // 剥类型参数：<T> / <T, U>
        if src[j..].starts_with('<') {
            let Some(gt) = src[j..].find('>') else { break };
            j += gt + 1;
        }
        let rest = src[j..].trim_start();
        if let Some(after) = rest.strip_prefix('(') {
            let after = after.trim_start();
            if let Some(name) = after.strip_prefix('\'') {
                let Some(end) = name.find('\'') else { break };
                out.insert(name[..end].to_string());
                i = j + 1;
                continue;
            }
        }
        i = j;
    }
    out
}

/// 扫 `ROUTED` 常量里的字符串字面量。
fn parse_routed(src: &str) -> BTreeSet<String> {
    let start = src.find("pub const ROUTED").expect("ROUTED const exists");
    let end = src[start..].find("];").expect("ROUTED list closes") + start;
    src[start..end]
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix('"')?
                .strip_suffix("\",")
                .map(str::to_string)
        })
        .collect()
}

/// transport 内部通道（客户端引擎直接 invoke，不经 client.ts 的 ipc.* 面）。
const TRANSPORT_INTERNAL: &[&str] = &["player_report"];

/// 迁移矩阵 C 桶 + W3 未下沉域（桌面专属/编排未就绪）：合法地不在 ROUTED 的
/// UI 调用（web 形态不提供，桌面壳承载）。
const DESKTOP_ONLY: &[&str] = &[
    "pick_folder",
    "library_choose_root",
    "library_reveal_track",
    "spectrum_set",
    "player_set_eq",
    "player_get_eq",
    "app_open_data_dir",
    "app_open_url",
    "classify_paths",
    "player_open_paths",
    "live_open",
    "live_pick_playlist",
    // 歌单导入/导出走系统文件选择器（导出写盘 / 导入读文件）；web 形态需要
    // 上传/下载 API 承接，随 W4+ 批次评估
    "playlist_export",
    "playlist_import",
    // W3 未路由的 beets 域余量（open_inbox/reveal 是桌面专属；enrich_index
    // 系列随长任务批次逐条评估进 ROUTED）
    "beets_open_inbox",
    "beets_reveal",
];

#[test]
fn every_routed_command_is_called_by_the_ui() {
    let routed = parse_routed(IPC_RS);
    assert!(
        routed.len() >= 50,
        "routed set looks truncated: {}",
        routed.len()
    );
    let client = parse_client_calls(CLIENT_TS);
    let orphans: Vec<_> = routed
        .difference(&client)
        .filter(|c| !TRANSPORT_INTERNAL.contains(&c.as_str()))
        .collect();
    assert!(
        orphans.is_empty(),
        "server 路由了 UI 从不调用的命令（迁移矩阵外私货）: {orphans:?}"
    );
}

/// 反向覆盖：UI 调用的每条命令要么在 ROUTED、要么显式登记为桌面专属——
/// 缺席即 web 形态 404（app_set_settings 漏路由事故的回归闸）。
#[test]
fn every_ui_command_is_routed_or_desktop_only() {
    let routed = parse_routed(IPC_RS);
    let client = parse_client_calls(CLIENT_TS);
    let missing: Vec<_> = client
        .difference(&routed)
        .filter(|c| !TRANSPORT_INTERNAL.contains(&c.as_str()))
        .filter(|c| !DESKTOP_ONLY.contains(&c.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "UI 调用了未路由且非桌面专属的命令（web 形态 404）: {missing:?}"
    );
}

#[test]
fn desktop_only_commands_are_not_routed() {
    let routed = parse_routed(IPC_RS);
    // 出现在 ROUTED 即越界
    let leaked: Vec<_> = DESKTOP_ONLY
        .iter()
        .filter(|c| routed.contains(**c))
        .collect();
    assert!(
        leaked.is_empty(),
        "桌面专属命令进了 server 路由表: {leaked:?}"
    );
}
