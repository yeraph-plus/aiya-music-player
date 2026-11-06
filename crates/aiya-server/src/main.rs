// Web 版服务端壳：axum HTTP 壳 + 共享 crates 组装（方案与批次见批次文档）。
// 命令面按域路由（ipc.rs，契约守卫见 tests/contract.rs）；音频出声在浏览器，
// server 只做字节级 IO；装载意图经 `state.token` 变化下发到客户端引擎。

mod auth;
mod enrich;
mod events;
mod host;
mod ipc;
mod media;
mod scan;
mod state;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::Router;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::state::ServerState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            // 与全仓惯例对齐（src-tauri/logging.rs、audio-host）：AIYA_LOG 优先，
            // RUST_LOG 兜底
            tracing_subscriber::EnvFilter::try_from_env("AIYA_LOG")
                .or_else(|_| tracing_subscriber::EnvFilter::try_from_default_env())
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_root = PathBuf::from(
        std::env::var("AIYA_DATA_ROOT").unwrap_or_else(|_| "./aiya-server-data".into()),
    );
    std::fs::create_dir_all(&data_root).expect("create data root");
    let db_path = data_root.join("library.sqlite");
    let db = aiya_data::open(&db_path).expect("open library db");

    let settings = db
        .send(|conn| Ok(state::setting_get(conn, "app_settings")))
        .ok()
        .flatten()
        .unwrap_or_else(|| serde_json::from_str(state::DEFAULT_SETTINGS).expect("valid default"));

    let st = Arc::new(ServerState {
        auth: auth::AuthGate::new(
            std::env::var("AIYA_ADMIN_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
            std::env::var("AIYA_TOKEN").ok().filter(|s| !s.is_empty()),
        ),
        events: events::EventBus::new(),
        scan: scan::ScanCtl::new(),
        beets_install: std::sync::Arc::new(aiya_host_core::InstallRegistry::new()),
        last_played: Mutex::new((String::new(), 0)),
        last_persist_ms: std::sync::atomic::AtomicU64::new(0),
        db: Mutex::new(db),
        db_path: db_path.clone(),
        artwork_dir: data_root.join("cache").join("artwork"),
        beets_index_dir: data_root.join("beets-index"),
        player: Mutex::new(aiya_player::PlayerActor::new()),
        settings: Mutex::new(settings),
        enrich_store: std::sync::OnceLock::new(),
    });
    for dir in [
        st.artwork_dir.join("originals"),
        st.artwork_dir.join("thumbs"),
    ] {
        let _ = std::fs::create_dir_all(dir);
    }

    // 启动扫描（可选）：AIYA_MUSIC_DIR 仅作首启种子（曲库根权威在应用内配置），
    // 全新库时写进设置并增量入库；既有库一律按设置里的根扫描。空串视为未设
    //（容器里显式 `AIYA_MUSIC_DIR=` 是"零根启动"的合法表达，不能把空路径写进种子）
    if let Some(music) = std::env::var("AIYA_MUSIC_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        match state::bootstrap_scan(&st, PathBuf::from(music).as_path()) {
            Ok(()) => {}
            Err(e) => tracing::error!("bootstrap scan failed: {e}"),
        }
    }
    state::restore_queue(&st);
    scan::spawn_timer(Arc::clone(&st));

    let port: u16 = std::env::var("AIYA_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);
    let bind = std::env::var("AIYA_BIND").unwrap_or_else(|_| "127.0.0.1".into());

    let mut app = Router::new()
        .route("/ipc/{cmd}", post(ipc::invoke))
        .route("/events", get(events_sse))
        .route("/audio/{id}", get(media::audio_by_id))
        .route("/artwork/{id}", get(media::artwork_by_id))
        .route("/stream", get(media::stream_proxy))
        .route("/__spike", get(spike_page))
        .route("/__spike/hls.js", get(spike_hls_js))
        .route("/", get(index_injected))
        .route("/__aiya_boot.js", get(boot_js))
        .route("/auth/login", post(auth::login))
        .route("/auth/state", get(auth::state_probe))
        .route("/auth/logout", post(auth::logout));
    if let Some(dir) = std::env::var("AIYA_FIXTURES_DIR").ok().map(PathBuf::from) {
        app = app.nest_service("/fixtures", ServeDir::new(dir));
    }
    if let Some(dir) = std::env::var("AIYA_HLS_DIR").ok().map(PathBuf::from) {
        app = app.nest_service("/hls", ServeDir::new(dir));
    }
    app = match std::env::var("AIYA_STATIC_DIR").ok().map(PathBuf::from) {
        Some(dir) => app.fallback_service(ServeDir::new(dir)),
        None => app.fallback_service(get(|| async { "aiya-server: no UI (set AIYA_STATIC_DIR)" })),
    };
    // CORS：默认**不挂**——浏览器同源访问不需要 CORS，permissive 等于把整张
    // 命令面（含无门时的 library_clear）开放给任意网页的 drive-by 读取。
    // 跨域场景（反代域不同源、开发期 vite dev server）用 AIYA_CORS_ORIGIN
    // 显式开白名单（逗号分隔 origin 列表）。
    let cors_origins: Vec<_> = std::env::var("AIYA_CORS_ORIGIN")
        .ok()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let app = if cors_origins.is_empty() {
        app.layer(middleware::from_fn_with_state(st.clone(), auth::middleware))
            .with_state(st)
    } else {
        tracing::info!("CORS allowlist enabled: {:?}", cors_origins);
        app.layer(middleware::from_fn_with_state(st.clone(), auth::middleware))
            .layer(
                CorsLayer::new()
                    .allow_origin(cors_origins)
                    .allow_methods([axum::http::Method::GET, axum::http::Method::POST]),
            )
            .with_state(st)
    };

    let listener = tokio::net::TcpListener::bind((bind.as_str(), port))
        .await
        .expect("bind");
    tracing::info!("aiya-server listening on http://{bind}:{port}");
    axum::serve(listener, app).await.expect("serve");
}

/// SSE：连接建立即推最近一次扫描快照（不逐帧重放），此后订阅事件总线——
/// scan:progress + beets×5 的生产者在命令与后台线程里。player:state /
/// spectrum:frame 不上 SSE：进度时钟在浏览器，快照由客户端合成。
async fn events_sse(
    State(st): State<Arc<ServerState>>,
) -> Sse<
    axum::response::sse::KeepAliveStream<
        ReceiverStream<Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
> {
    let (tx, rx) = tokio::sync::mpsc::channel::<
        Result<axum::response::sse::Event, std::convert::Infallible>,
    >(64);
    // 锁只护取值：MutexGuard 非 Send，不得跨下方 send 的挂起点
    let snap = st
        .scan
        .last
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(snap) = snap {
        let _ = tx
            .send(Ok(axum::response::sse::Event::default()
                .event("scan:progress")
                .data(serde_json::to_string(&snap).unwrap_or_default())))
            .await;
    }
    tokio::spawn(async move {
        let mut rx = st.events.subscribe();
        loop {
            // 客户端断连不再依赖"下一个事件到达"才察觉：tx.closed() 在
            // 响应体被 drop 时立刻就绪，静默期任务也能退出（审查 P2）
            let ev = tokio::select! {
                ev = rx.recv() => ev,
                _ = tx.closed() => break,
            };
            match ev {
                Ok((channel, data)) => {
                    let ev = axum::response::sse::Event::default()
                        .event(channel)
                        .data(data);
                    if tx.send(Ok(ev)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    Sse::new(ReceiverStream::new(rx)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// 首页注入引导标志：同源服务的 UI 以 `window.__AIYA_BOOT__ = 'http'` 起动
/// http transport（vite preview / e2e 无此标志 → mock 路径，零探测请求）。
async fn index_injected() -> Response {
    let Ok(dir) = std::env::var("AIYA_STATIC_DIR") else {
        return (StatusCode::NOT_FOUND, "no UI (set AIYA_STATIC_DIR)").into_response();
    };
    let path = std::path::PathBuf::from(dir).join("index.html");
    let Ok(html) = std::fs::read_to_string(&path) else {
        return (StatusCode::NOT_FOUND, "index.html missing").into_response();
    };
    let injected = html.replacen(
        "<head>",
        "<head><script src=\"/__aiya_boot.js\"></script>",
        1,
    );
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        injected,
    )
        .into_response()
}

async fn boot_js() -> Response {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        "window.__AIYA_BOOT__ = 'http';",
    )
        .into_response()
}

/// 浏览器手工实测页（spike）：Range seek 与代理清单的 <audio>/hls.js 行为。
/// hls.js 从 AIYA_HLS_JS 指定的本地路径读取，不进仓。
async fn spike_page() -> Html<&'static str> {
    Html(include_str!("../spike/test.html"))
}

async fn spike_hls_js() -> Response {
    match std::env::var("AIYA_HLS_JS") {
        Ok(p) => match std::fs::read(&p) {
            Ok(bytes) => {
                ([(header::CONTENT_TYPE, "application/javascript")], bytes).into_response()
            }
            Err(e) => (StatusCode::NOT_FOUND, format!("{e}")).into_response(),
        },
        Err(_) => (StatusCode::NOT_FOUND, "AIYA_HLS_JS not set").into_response(),
    }
}
