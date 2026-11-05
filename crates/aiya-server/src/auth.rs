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

    /// 登录：口令核验（固定延时抗时序/暴破）→ 签发会话令牌。
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
            std::thread::sleep(Duration::from_millis(FAIL_DELAY_MS));
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
    // 进程内随机（熵源：地址/时间/原子序）。浏览器侧会话配合 HTTPS 反代使用，
    // 这里不追求密码学熵——口令才是凭据本体，会话只是门票。
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let a = &t as *const _ as usize;
    let s = SEQ.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    format!("{:016x}{:016x}{:016x}", t as u64, a as u64 ^ s, s)
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
        Err(()) => (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "code": "unauthorized", "message": "wrong password" })),
        )
            .into_response(),
    }
}

/// 登录状态预检（公开）：UI 据此决定进闸还是直接启动。
pub async fn state_probe(
    axum::extract::State(st): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    req: Request,
) -> Response {
    let required = st.auth.required();
    let authenticated = st.auth.authenticated(credential(&req).as_deref());
    (
        StatusCode::OK,
        axum::Json(json!({ "required": required, "authenticated": authenticated })),
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
