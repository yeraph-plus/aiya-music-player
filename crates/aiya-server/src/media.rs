// 媒体面：/audio（曲库文件 Range 流式）、/artwork（封面缓存）、/stream（外部
// HLS 字节代理）。常规播放 server 零解码——字节级 IO，Symphonia 不进编译图。

use std::collections::HashMap;
use std::io::Read as _;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt as _;
use tower_http::services::ServeFile;
use url::Url;

use crate::state::{db_lock, ServerState};

/// `/audio/{trackId}`：id 查库取 path → Range 流式。库内行不存在 = notFound。
pub async fn audio_by_id(
    State(st): State<std::sync::Arc<ServerState>>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let path: Option<String> = db_lock(&st)
        .send(move |conn| {
            Ok(conn
                .query_row("SELECT path FROM tracks WHERE id = ?1", [&id], |r| r.get(0))
                .ok())
        })
        .unwrap_or(None);
    let Some(path) = path else {
        return err_json(
            StatusCode::NOT_FOUND,
            aiya_dto::ApiError::not_found("track not found"),
        );
    };
    serve_file(std::path::PathBuf::from(path), req).await
}

/// `/artwork/{id}?size=N`：只按 id 查缓存（C5 的 HTTP 形态）。thumbs 缺席时
/// 回落原图（W3 起补按需缩放）；未登记的 id → notFound，不落盘。
pub async fn artwork_by_id(
    State(st): State<std::sync::Arc<ServerState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
    req: Request,
) -> Response {
    let row: Option<(String, String)> = db_lock(&st)
        .send(move |conn| aiya_data::artwork::find_artwork(conn, &id))
        .unwrap_or(None);
    let Some((cache_key, _mime)) = row else {
        return err_json(
            StatusCode::NOT_FOUND,
            aiya_dto::ApiError::not_found("artwork not found"),
        );
    };
    let size = q.get("size").cloned().unwrap_or_default();
    let thumb = st
        .artwork_dir
        .join("thumbs")
        .join(format!("{cache_key}_{size}.jpg"));
    let file = if size.is_empty() || !thumb.exists() {
        // 原图扩展名在 cache_key 里（扫描期写入）
        st.artwork_dir.join("originals").join(&cache_key)
    } else {
        thumb
    };
    serve_file(file, req).await
}

async fn serve_file(path: std::path::PathBuf, req: Request) -> Response {
    if !path.is_file() {
        return err_json(
            StatusCode::NOT_FOUND,
            aiya_dto::ApiError::not_found("media file missing on disk"),
        );
    }
    match ServeFile::new(path).oneshot(req).await {
        Ok(resp) => resp.map(Body::new),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serve failed: {e}"),
        )
            .into_response(),
    }
}

fn err_json(status: StatusCode, e: aiya_dto::ApiError) -> Response {
    match serde_json::to_value(e) {
        Ok(v) => (status, axum::Json(v)).into_response(),
        Err(_) => (status, "error serialization failed").into_response(),
    }
}

/// `/stream?u=<url>`：HLS 字节代理。清单逐行换源到本代理（token 透传），
/// 段与普通字节透传；MIME 显式给。
pub async fn stream_proxy(Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(target) = q.get("u") else {
        return (StatusCode::BAD_REQUEST, "missing u").into_response();
    };
    let Ok(base) = Url::parse(target) else {
        return (StatusCode::BAD_REQUEST, "bad url").into_response();
    };
    let is_playlist = target.ends_with(".m3u8") || target.ends_with(".m3u");

    let fetch = {
        let target = target.clone();
        tokio::task::spawn_blocking(move || {
            ureq::get(&target)
                .timeout(Duration::from_secs(15))
                .call()
                .map_err(|e| e.to_string())
        })
        .await
    };
    let resp = match fetch {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return (StatusCode::BAD_GATEWAY, format!("upstream: {e}")).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };

    if is_playlist {
        let text = match resp.into_string() {
            Ok(t) => t,
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("read: {e}")).into_response(),
        };
        let mut out = String::with_capacity(text.len() + 256);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                out.push_str(line);
            } else {
                match base.join(trimmed) {
                    Ok(resolved) => {
                        // token 透传：hls.js 的段请求 URL 由清单给定，查询参数是
                        // <audio>/hls.js 都能携带的凭据形态（W4 鉴权设计留待定案）
                        let mut ser = url::form_urlencoded::Serializer::new(String::new());
                        ser.append_pair("u", resolved.as_str());
                        if let Some(tok) = q.get("token") {
                            ser.append_pair("token", tok);
                        }
                        out.push_str("/stream?");
                        out.push_str(&ser.finish());
                    }
                    Err(_) => out.push_str(trimmed),
                }
            }
            out.push('\n');
        }
        (
            [
                (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            out,
        )
            .into_response()
    } else {
        let mime = segment_mime(target);
        let mut bytes = Vec::new();
        if resp.into_reader().read_to_end(&mut bytes).is_err() {
            return (StatusCode::BAD_GATEWAY, "segment read failed").into_response();
        }
        ([(header::CONTENT_TYPE, mime)], bytes).into_response()
    }
}

fn segment_mime(url: &str) -> &'static str {
    let lower = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if lower.ends_with(".ts") {
        "video/mp2t"
    } else if lower.ends_with(".aac") {
        "audio/aac"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".m4s") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else {
        "application/octet-stream"
    }
}
