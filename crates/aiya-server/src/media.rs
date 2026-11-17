// 媒体面：/audio（曲库文件 Range 流式）、/artwork（封面缓存）、/stream（外部
// HLS 字节代理）。常规播放 server 零解码——字节级 IO，Symphonia 不进编译图。

use std::collections::HashMap;
use std::io::Read as _;
use std::net::ToSocketAddrs;
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

/// 段读取上限：正常 HLS 段 ≤ 8MB（与 aiya-audio/hls.rs 的 SEGMENT_MAX_BYTES
/// 同量级），超限视为异常目标，拒绝吃满内存（审查 P1：read_to_end 无上限）。
const SEGMENT_READ_CAP: u64 = 64 * 1024 * 1024;

/// 代理目标校验：scheme 白名单 + 拒绝环回/私网/link-local 目标。/stream 是
/// 服务端发起的抓取器，不做这层就是内网探测与云元数据端点（169.254.169.254）
/// 的读取跳板（审查 P1 SSRF）。DNS 解析在调用方的阻塞线程里做。
fn validate_target(base: &Url) -> Result<Vec<std::net::SocketAddr>, String> {
    if base.scheme() != "http" && base.scheme() != "https" {
        return Err(format!("scheme not allowed: {}", base.scheme()));
    }
    let host = base.host_str().unwrap_or_default();
    let port = base.port_or_known_default().unwrap_or(80);
    let addrs: Vec<std::net::SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve failed: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err("host did not resolve".into());
    }
    for a in &addrs {
        let ip = a.ip();
        // 手写字节判定（is_shared/is_unique_local 尚未 stable）：环回/私网/
        // link-local/未指定/广播/0.0.0.0/8、CGNAT 100.64/10、组播与保留段、
        // IPv6 fc00::/7 与 fe80::/10、组播
        let denied = match ip.to_canonical() {
            std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || o[0] == 0
                    || (o[0] == 100 && (o[1] & 0xc0) == 0x40)
                    || o[0] >= 224
            }
            std::net::IpAddr::V6(v6) => {
                let s = v6.segments();
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (s[0] & 0xfe00) == 0xfc00
                    || (s[0] & 0xffc0) == 0xfe80
                    || (s[0] & 0xff00) == 0xff00
            }
        };
        if denied {
            return Err(format!("target resolves to non-public address ({ip})"));
        }
    }
    Ok(addrs)
}

fn fetch_public(target: Url) -> Result<(Url, ureq::Response), String> {
    fetch_redirects(target, validate_target)
}

fn fetch_redirects(
    mut target: Url,
    resolve: impl Fn(&Url) -> Result<Vec<std::net::SocketAddr>, String>,
) -> Result<(Url, ureq::Response), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    for hop in 0..=5 {
        let addrs = resolve(&target)?;
        // 请求使用已校验的地址集合，避免校验与连接之间再次解析 DNS。
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .resolver(move |_: &str| Ok(addrs.clone()))
            .timeout(
                deadline
                    .checked_duration_since(std::time::Instant::now())
                    .ok_or("upstream timeout")?,
            )
            .build();
        let response = agent
            .get(target.as_str())
            .call()
            .map_err(|e| e.to_string())?;
        if !matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            return Ok((target, response));
        }
        if hop == 5 {
            return Err("too many upstream redirects".into());
        }
        let location = response
            .header("Location")
            .ok_or("redirect without Location")?;
        target = target.join(location).map_err(|e| e.to_string())?;
    }
    unreachable!()
}

fn rewrite_playlist(text: &str, base: &Url, mut rewrite: impl FnMut(Url) -> String) -> String {
    let mut out = String::with_capacity(text.len() + 256);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            let mut rest = line;
            while let Some(i) = rest.find("URI=\"") {
                out.push_str(&rest[..i + 5]);
                rest = &rest[i + 5..];
                let Some(end) = rest.find('"') else {
                    break;
                };
                match base.join(&rest[..end]) {
                    Ok(url) => out.push_str(&rewrite(url)),
                    Err(_) => out.push_str(&rest[..end]),
                }
                out.push('"');
                rest = &rest[end + 1..];
            }
            out.push_str(rest);
        } else if !line.is_empty() {
            match base.join(line) {
                Ok(url) => out.push_str(&rewrite(url)),
                Err(_) => out.push_str(line),
            }
        }
        out.push('\n');
    }
    out
}

/// 私有站单把凭据带入子清单和段请求，原生 HLS 与 MSE 使用同一份清单。
pub async fn hls_file(
    State(st): State<std::sync::Arc<ServerState>>,
    Path((dir, file)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    req: Request,
) -> Response {
    let safe = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !safe(&dir) || !safe(&file) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(root) = st.station.hls_root() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = root.join(&dir).join(&file);
    if !file.ends_with(".m3u8") {
        return serve_file(path, req).await;
    }
    let text = match tokio::fs::read_to_string(path).await {
        Ok(t) => t,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let base =
        Url::parse(&format!("http://localhost/hls/{dir}/{file}")).expect("validated HLS path");
    let body = rewrite_playlist(&text, &base, |mut url| {
        if let Some(token) = q.get("token") {
            url.query_pairs_mut().append_pair("token", token);
        }
        format!(
            "{}{}",
            url.path(),
            url.query().map(|s| format!("?{s}")).unwrap_or_default()
        )
    });
    (
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
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
    let fetch = { tokio::task::spawn_blocking(move || fetch_public(base)).await };
    let (base, resp) = match fetch {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return (StatusCode::BAD_GATEWAY, format!("upstream: {e}")).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };

    let path_lower = base.path().to_ascii_lowercase();
    let content_type = resp
        .header("Content-Type")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_playlist = path_lower.ends_with(".m3u8")
        || path_lower.ends_with(".m3u")
        || content_type.contains("mpegurl");

    if is_playlist {
        let text = match resp.into_string() {
            Ok(t) => t,
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("read: {e}")).into_response(),
        };
        let out = rewrite_playlist(&text, &base, |resolved| {
            let mut ser = url::form_urlencoded::Serializer::new(String::new());
            ser.append_pair("u", resolved.as_str());
            if let Some(tok) = q.get("token") {
                ser.append_pair("token", tok);
            }
            format!("/stream?{}", ser.finish())
        });
        (
            [
                (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            out,
        )
            .into_response()
    } else {
        let mime = segment_mime(base.as_str());
        let mut bytes = Vec::new();
        let mut reader = resp.into_reader().take(SEGMENT_READ_CAP);
        if reader.read_to_end(&mut bytes).is_err() {
            return (StatusCode::BAD_GATEWAY, "segment read failed").into_response();
        }
        if bytes.len() as u64 >= SEGMENT_READ_CAP {
            return (
                StatusCode::BAD_GATEWAY,
                "segment exceeds read cap; refusing",
            )
                .into_response();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn public_targets_reject_private_and_unsupported_addresses() {
        for url in [
            "http://127.0.0.1/a",
            "http://10.0.0.1/a",
            "http://169.254.169.254/",
            "http://[::ffff:127.0.0.1]/",
            "file:///etc/passwd",
        ] {
            assert!(validate_target(&Url::parse(url).unwrap()).is_err(), "{url}");
        }
        assert!(validate_target(&Url::parse("https://8.8.8.8/a").unwrap()).is_ok());
    }

    #[test]
    fn redirect_to_private_target_is_checked_before_connecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0; 4096];
            assert!(s.read(&mut buf).unwrap() > 0);
            s.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let result = fetch_redirects(Url::parse("http://public.test/live").unwrap(), |url| {
            if url.host_str() == Some("public.test") {
                Ok(vec![addr])
            } else {
                validate_target(url)
            }
        });
        server.join().unwrap();
        assert!(result.unwrap_err().contains("non-public"));
    }

    #[test]
    fn playlists_rewrite_segments_keys_maps_and_variant_playlists() {
        let base = Url::parse("https://example.com/redirected/live.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXT-X-MAP:URI=\"init.mp4\"\nvariant.m3u8\nseg.ts\n";
        let rewritten = rewrite_playlist(text, &base, |url| format!("proxy:{}", url));
        for name in ["key.bin", "init.mp4", "variant.m3u8", "seg.ts"] {
            assert!(
                rewritten.contains(&format!("proxy:https://example.com/redirected/{name}")),
                "{rewritten}"
            );
        }
    }
}
