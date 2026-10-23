//! beets sidecar 宿主侧：解释器探测 + status 查询（docs/08-beets.md B0）。
//!
//! 探测顺序：AIYA_BEETS_PYTHON env → dev venv（仅 debug 构建）→ 系统 python。
//! sidecar 脚本 include_str! 进二进制，写到 app_data/sidecar/（dev/release 行为一致）。
//! v1 不捆绑 Python/beets：没装就返回 not_found，设置页给安装指引，播放功能不受影响。

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// 设置页 beets 状态区数据源。serde 进 IPC：camelCase 契约（铁律）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeetsStatus {
    pub found: bool,
    pub interpreter: Option<String>,
    pub script_path: Option<String>,
    pub protocol_version: Option<u32>,
    pub beets_version: Option<String>,
    pub python_version: Option<String>,
    pub library_path: Option<String>,
    pub library_exists: Option<bool>,
    pub plugins: Option<Vec<String>>,
    pub config_dir: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

const SIDECAR_SCRIPT: &str = include_str!("../../sidecar/beets_sidecar.py");

/// sidecar 脚本落盘位置：{app_data}/sidecar/beets_sidecar.py。
/// 内容与内嵌不同才重写（升级友好）。
fn ensure_script(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("sidecar");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir: {e}"))?;
    let path = dir.join("beets_sidecar.py");
    let mismatch = std::fs::read_to_string(&path).map(|old| old != SIDECAR_SCRIPT).unwrap_or(true);
    if mismatch {
        std::fs::write(&path, SIDECAR_SCRIPT).map_err(|e| format!("write sidecar: {e}"))?;
    }
    Ok(path)
}

/// 候选解释器列表（按优先级）。
fn interpreter_candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("AIYA_BEETS_PYTHON") {
        out.push(p);
    }
    // dev venv：release 构建不存在，自然跳过
    let dev_venv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/beets-venv/Scripts/python.exe");
    out.push(dev_venv.to_string_lossy().into_owned());
    out.push("python".into());
    out
}

/// 向解释器发一条 status/ping 请求并等响应（行帧 JSON，超时保护）。
fn query_sidecar(
    python: &str,
    script: &str,
    timeout: Duration,
) -> Result<BeetsStatus, String> {
    let mut child = std::process::Command::new(python)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {python}: {e}"))?;

    // 写请求（drop stdin 让读循环可以结束——本命令是一次性查询）
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = writeln!(stdin, r#"{{"id":1,"cmd":"status"}}"#);
    }

    // 读响应：单独线程 + 超时，避免 beets 卡住拖死宿主
    let (tx, rx) = std::sync::mpsc::channel();
    let mut stdout = child.stdout.take().expect("stdout piped");
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(&mut stdout);
        for line in reader.lines() {
            let _ = tx.send(line.unwrap_or_default());
            break; // status 是单问单答
        }
    });
    let line = rx
        .recv_timeout(timeout)
        .map_err(|_| format!("sidecar 响应超时（{timeout:.0?}）"))?;
    let _ = child.kill();
    let _ = child.wait();

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        ok: bool,
        data: Option<serde_json::Value>,
        error: Option<SidecarError>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SidecarError {
        code: String,
        stage: String,
        beets_message: String,
    }

    let env: Envelope =
        serde_json::from_str(line.trim()).map_err(|e| format!("解析 sidecar 响应: {e}"))?;
    if !env.ok {
        let err = env.error.unwrap_or(SidecarError {
            code: "E_UNKNOWN".into(),
            stage: "unknown".into(),
            beets_message: "no error detail".into(),
        });
        return Ok(BeetsStatus {
            found: false,
            interpreter: Some(python.into()),
            script_path: Some(script.into()),
            protocol_version: None,
            beets_version: None,
            python_version: None,
            library_path: None,
            library_exists: None,
            plugins: None,
            config_dir: None,
            error_code: Some(err.code),
            error_message: Some(format!("[{}] {}", err.stage, err.beets_message)),
        });
    }
    let data = env.data.unwrap_or(serde_json::Value::Null);
    Ok(BeetsStatus {
        found: true,
        interpreter: Some(python.into()),
        script_path: Some(script.into()),
        protocol_version: data.get("protocolVersion").and_then(|v| v.as_u64()).map(|v| v as u32),
        beets_version: data.get("beetsVersion").and_then(|v| v.as_str()).map(Into::into),
        python_version: data.get("pythonVersion").and_then(|v| v.as_str()).map(Into::into),
        library_path: data.get("libraryPath").and_then(|v| v.as_str()).map(Into::into),
        library_exists: data.get("libraryExists").and_then(|v| v.as_bool()),
        plugins: data
            .get("plugins")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(Into::into)).collect()),
        config_dir: data.get("configDir").and_then(|v| v.as_str()).map(Into::into),
        error_code: None,
        error_message: None,
    })
}

/// 完整探测：按候选解释器逐个试，第一个能给出 status 的胜出。
/// 全部失败 → not_found（有 error_message 供设置页显示）。
pub fn probe(app: &tauri::AppHandle) -> BeetsStatus {
    let script = match ensure_script(app) {
        Ok(p) => p,
        Err(e) => {
            return BeetsStatus {
                found: false,
                interpreter: None,
                script_path: None,
                protocol_version: None,
                beets_version: None,
                python_version: None,
                library_path: None,
                library_exists: None,
                plugins: None,
                config_dir: None,
                error_code: Some("E_SCRIPT".into()),
                error_message: Some(e),
            };
        }
    };
    let script_str = script.to_string_lossy().into_owned();
    let mut last_err: Option<String> = None;
    for python in interpreter_candidates() {
        match query_sidecar(&python, &script_str, Duration::from_secs(10)) {
            Ok(status) => {
                if status.found {
                    return status;
                }
                // 解释器可用但没装 beets：继续试下一个候选（dev venv 优先命中）
                last_err = status.error_message;
            }
            Err(e) => last_err = Some(e),
        }
    }
    BeetsStatus {
        found: false,
        interpreter: None,
        script_path: Some(script_str),
        protocol_version: None,
        beets_version: None,
        python_version: None,
        library_path: None,
        library_exists: None,
        plugins: None,
        config_dir: None,
        error_code: Some("E_BEETS_NOT_FOUND".into()),
        error_message: last_err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 形状锚点：BeetsStatus 是设置页数据源，必须 camelCase 进 IPC。
    #[test]
    fn beets_status_serializes_camel_case() {
        // BeetsStatus 只有 Deserialize；用 JSON 反序列化锚定输入形状
        let v = serde_json::json!({
            "found": true, "interpreter": "py", "scriptPath": "/s.py",
            "protocolVersion": 1, "beetsVersion": "2.14.1", "pythonVersion": "3.14.7",
            "libraryPath": "l.db", "libraryExists": true,
            "plugins": ["musicbrainz"], "configDir": "c",
            "errorCode": null, "errorMessage": null
        });
        let st: BeetsStatus = serde_json::from_value(v).expect("camelCase 形状必须可解析");
        assert_eq!(st.beets_version.as_deref(), Some("2.14.1"));
        assert_eq!(st.library_exists, Some(true));
        assert_eq!(st.plugins, Some(vec!["musicbrainz".to_string()]));
    }

    /// dev venv 存在时（本仓库），probe 能找到 beets 2.14.x。
    #[cfg(debug_assertions)]
    #[test]
    fn probe_finds_dev_venv_beets() {
        let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/beets-venv/Scripts/python.exe");
        if !venv.exists() {
            return; // CI/其他机器没有 venv 就跳过
        }
        let status = probe_with(&venv.to_string_lossy());
        assert!(status.found, "dev venv 应命中 beets: {:?}", status.error_message);
        assert!(status.beets_version.as_deref().unwrap_or_default().starts_with("2.14"));
        assert!(status.plugins.is_some());
    }

    /// probe 的可注入版本：probe() 就是用候选列表循环 probe_with。
    fn probe_with(python: &str) -> BeetsStatus {
        // 复用 query_sidecar 的完整逻辑，脚本用仓库源（不依赖 app_data）
        match query_sidecar(python, &script_path_for_test(), Duration::from_secs(10)) {
            Ok(s) => s,
            Err(e) => BeetsStatus {
                found: false,
                interpreter: Some(python.into()),
                script_path: None,
                protocol_version: None,
                beets_version: None,
                python_version: None,
                library_path: None,
                library_exists: None,
                plugins: None,
                config_dir: None,
                error_code: Some("E_SPAWN".into()),
                error_message: Some(e),
            },
        }
    }

    fn script_path_for_test() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../sidecar/beets_sidecar.py")
            .to_string_lossy()
            .into_owned()
    }
}
