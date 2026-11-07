// 登录门：单管理员模型（无多租户）。口令来自 `AIYA_ADMIN_PASSWORD`；
// 未设置 = 局域网无门模式（与既有 `AIYA_TOKEN` 静态令牌并行共存——后者
// 面向脚本/curl 场景，签发的会话面向浏览器）。
//
// 会话在内存里（进程重启全体失效，浏览器下次进闸重登）；滑动过期 7 天。
// 失败按固定延时回落，拖慢在线暴破；成功签发 32 字节随机令牌。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

const SESSION_TTL_MS: i64 = 7 * 24 * 3600 * 1000;
const FAIL_DELAY_MS: u64 = 600;

pub struct AuthGate {
    admin_password: Option<String>,
    static_token: Option<String>,
    sessions: Mutex<HashMap<String, i64>>,
}

impl AuthGate {
    pub fn new(admin_password: Option<String>, static_token: Option<String>) -> Self {
        Self {
            admin_password: admin_password.filter(|s| !s.is_empty()),
            static_token: static_token.filter(|s| !s.is_empty()),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 门是否开启（口令或静态令牌任一设置即开）。
    pub fn required(&self) -> bool {
        self.admin_password.is_some() || self.static_token.is_some()
    }

    pub fn authenticated(&self, token: Option<&str>) -> bool {
        match token {
            Some(t) => self.validate(t),
            None => !self.required(),
        }
    }

    /// 登录：口令核验（常数时间比较）→ 签发会话令牌。失败延时由 handler
    /// 以 tokio sleep 承担（本函数是同步路径，std sleep 会睡死 tokio worker）。
    pub fn login(&self, password: &str) -> Result<(String, i64), ()> {
        let Some(expected) = &self.admin_password else {
            return Err(());
        };
        let ok = expected.len() == password.len()
            && expected
                .bytes()
                .zip(password.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if !ok {
            return Err(());
        }
        let token = new_token();
        let expires = now_ms() + SESSION_TTL_MS;
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(token.clone(), expires);
        Ok((token, expires))
    }

    /// 会话或静态令牌任一命中即有效；过期会话顺手清理。
    pub fn validate(&self, token: &str) -> bool {
        if self.static_token.as_deref() == Some(token) {
            return true;
        }
        let now = now_ms();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match sessions.get(token) {
            Some(exp) if *exp > now => {
                // 滑动续期
                sessions.insert(token.to_string(), now + SESSION_TTL_MS);
                true
            }
            Some(_) => {
                sessions.remove(token);
                false
            }
            None => false,
        }
    }

    pub fn revoke(&self, token: &str) {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(token);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn new_token() -> String {
    // OS 熵源（getrandom）：会话令牌在 7 天滑动期内等价 API 凭据本体，
    // 不再用时间/地址/原子序拼凑（审查 P1：可预测性）
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS entropy unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 提取凭据：`Authorization: Bearer X` 优先，其次 `?token=X`。
fn credential(req: &Request) -> Option<String> {
    if let Some(v) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.to_string());
        }
    }
    req.uri()
        .query()
        .and_then(|q| q.split('&').find(|kv| kv.starts_with("token=")))
        .map(|kv| kv["token=".len()..].to_string())
}

/// 公开路径：静态壳与登录端点（登录门要能先加载出来），spike/fixture 试验位。
fn is_public(path: &str) -> bool {
    path == "/"
        || path.starts_with("/assets/")
        || path == "/__aiya_boot.js"
        || path.starts_with("/auth/")
        || path.starts_with("/__spike")
        || path.starts_with("/fixtures")
}

/// 统一鉴权中间件：门未开直接过；开了则凭据命中（会话/静态令牌）才放行。
pub async fn middleware(
    axum::extract::State(st): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    req: Request,
    next: Next,
) -> Response {
    if !st.auth.required() || is_public(req.uri().path()) {
        return next.run(req).await;
    }
    // 公开频道豁免：/hls/{dir}/* 在 stations.visibility =
    // public 时免凭据——豁免集在内存频道表里（Mutex 一次锁，不查 DB）。
    if is_public_station(req.uri().path(), &st.station) {
        return next.run(req).await;
    }
    match credential(&req) {
        Some(token) if st.auth.validate(&token) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "code": "unauthorized", "message": "login required" })),
        )
            .into_response(),
    }
}

/* ---- 登录端点（公开） ---- */

pub async fn login(
    axum::extract::State(st): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> Response {
    let password = body
        .get("password")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    match st.auth.login(password) {
        Ok((token, expires_at_ms)) => (
            StatusCode::OK,
            axum::Json(json!({ "token": token, "expiresAtMs": expires_at_ms })),
        )
            .into_response(),
        Err(()) => {
            // 失败回落：tokio sleep（不占 worker 线程）。抗时序/暴破的唯一
            // 限速就在这里，不能挪进同步路径（std sleep 会睡死 worker）
            tokio::time::sleep(Duration::from_millis(FAIL_DELAY_MS)).await;
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({ "code": "unauthorized", "message": "wrong password" })),
            )
                .into_response()
        }
    }
}

/// 登录状态预检（公开）：UI 据此决定进闸还是直接启动。`mode` 告知闸的
/// 凭据形态（password / token），登录卡据此切换输入语义——只发静态令牌
/// 的部署里口令输入是死路（login 端点恒 401）。
pub async fn state_probe(
    axum::extract::State(st): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    req: Request,
) -> Response {
    let required = st.auth.required();
    let authenticated = st.auth.authenticated(credential(&req).as_deref());
    let mode = if !required {
        None
    } else if st.auth.admin_password.is_some() {
        Some("password")
    } else {
        Some("token")
    };
    (
        StatusCode::OK,
        axum::Json(json!({
            "required": required,
            "authenticated": authenticated,
            "mode": mode,
        })),
    )
        .into_response()
}

pub async fn logout(
    axum::extract::State(st): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    req: Request,
) -> Response {
    if let Some(token) = credential(&req) {
        st.auth.revoke(&token);
    }
    (StatusCode::OK, axum::Json(json!({ "revoked": true }))).into_response()
}

/// 公开频道的流面豁免：/hls/{public_dir}/ 前缀（ws 接触点将来同挂这里）。
fn is_public_station(path: &str, station: &crate::station::StationCtl) -> bool {
    let Some(rest) = path.strip_prefix("/hls/") else {
        return false;
    };
    let dir = rest.split('/').next().unwrap_or_default();
    !dir.is_empty() && station.public_dirs().iter().any(|d| d == dir)
}
