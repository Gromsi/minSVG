//! Self-hosted optimize HTTP. Local by default; we do not host it.
//!
//! Built only with `--features serve` (tiny_http + serde_json, not axum).
//! Accidental public listens (`:8080`, `*`) are rewritten to loopback. Explicit
//! `0.0.0.0:PORT` is a container opt-in (no auth). `minsvg-mcp` is a separate
//! local stdio binary and never binds TCP.

use minsvg::optimize_str;
use std::io::Read;
use std::net::SocketAddr;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: u64 = 32 * 1024 * 1024;
const HEALTH_JSON: &str = "{\"ok\":true}";
pub const DEFAULT_BIND: &str = "127.0.0.1:8765";

pub fn run(bind: &str) -> Result<(), String> {
    let bind = resolve_bind(bind)?;
    let server = Server::http(&bind).map_err(|e| format!("bind {bind}: {e}"))?;
    let shown = match server.server_addr().to_ip() {
        Some(addr) => format!("http://{addr}"),
        None => format!("http://{bind}"),
    };
    if bind_is_unspecified(&bind) {
        eprintln!("minsvg serve listening on {shown} (not loopback; no auth; you deploy this)");
    } else {
        eprintln!("minsvg serve listening on {shown} (local; no auth)");
    }
    for request in server.incoming_requests() {
        if let Err(err) = handle(request) {
            eprintln!("minsvg serve: {err}");
        }
    }
    Ok(())
}

fn handle(mut request: Request) -> Result<(), String> {
    let method = request.method().clone();
    let path = normalize_path(request.url()).to_string();
    let content_type = content_type_header(&request);
    let response = match (method, path.as_str()) {
        (Method::Get, "/health") => json_ok(StatusCode::from(200), HEALTH_JSON.to_string()),
        (Method::Post, "/optimize") => optimize_response(&mut request, &content_type)?,
        (_, "/health" | "/optimize") => {
            json_ok(StatusCode::from(405), json_error("method not allowed"))
        }
        _ => json_ok(StatusCode::from(404), json_error("not found")),
    };
    request
        .respond(response)
        .map_err(|e| format!("respond: {e}"))
}

fn optimize_response(
    request: &mut Request,
    content_type: &str,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    let body = read_body(request)?;
    if body.len() as u64 > MAX_BODY {
        return Ok(json_ok(
            StatusCode::from(413),
            json_error("payload too large"),
        ));
    }
    let as_json = wants_json(content_type, &body);
    let svg = if as_json {
        match parse_svg_field(&body) {
            Ok(s) => s,
            Err(msg) => return Ok(json_ok(StatusCode::from(400), json_error(&msg))),
        }
    } else {
        match String::from_utf8(body) {
            Ok(s) => s,
            Err(_) => {
                return Ok(json_ok(
                    StatusCode::from(400),
                    json_error("body is not UTF-8"),
                ))
            }
        }
    };
    match optimize_str(&svg) {
        Ok(out) => {
            if as_json {
                let payload = serde_json::json!({ "data": out.svg }).to_string();
                Ok(json_ok(StatusCode::from(200), payload))
            } else {
                Ok(svg_ok(out.svg))
            }
        }
        Err(err) => Ok(json_ok(StatusCode::from(400), json_error(&err.to_string()))),
    }
}

fn parse_svg_field(body: &[u8]) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid JSON: {e}"))?;
    value
        .get("svg")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "missing svg field".into())
}

fn read_body(request: &mut Request) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("read body: {e}"))?;
    Ok(body)
}

fn wants_json(content_type: &str, body: &[u8]) -> bool {
    if media_type(content_type) == "application/json" {
        return true;
    }
    if is_svg_media(content_type) {
        return false;
    }
    body.iter().find(|b| !b.is_ascii_whitespace()).copied() == Some(b'{')
}

fn is_svg_media(content_type: &str) -> bool {
    let media = media_type(content_type);
    media == "image/svg+xml" || media == "image/svg"
}

fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or("").trim()
}

fn normalize_path(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or("/");
    if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    }
}

fn content_type_header(request: &Request) -> String {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Content-Type"))
        .map(|h| h.value.as_str().to_ascii_lowercase())
        .unwrap_or_default()
}

fn json_error(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

fn json_ok(status: StatusCode, body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(status)
        .with_header(ascii_header(
            "Content-Type",
            "application/json; charset=utf-8",
        ))
}

fn svg_ok(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode::from(200))
        .with_header(ascii_header("Content-Type", "image/svg+xml; charset=utf-8"))
}

fn ascii_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("ascii header")
}

/// Loopback by default. Port-only / wildcard hosts become `127.0.0.1`.
/// Bare `0.0.0.0` / LAN IPs without a port are refused. `0.0.0.0:PORT` stays
/// an explicit container bind.
pub fn resolve_bind(bind: &str) -> Result<String, String> {
    let bind = bind.trim();
    if bind.is_empty() {
        return Ok(DEFAULT_BIND.to_string());
    }
    if let Ok(addr) = bind.parse::<SocketAddr>() {
        return Ok(addr.to_string());
    }
    if is_loopback_host(bind) {
        return Ok(loopback_with_default_port(bind));
    }
    if let Some(port) = parse_port_only(bind) {
        return Ok(format!("127.0.0.1:{port}"));
    }
    if let Some((host, port)) = split_host_port(bind) {
        if host.is_empty() || host == "*" || host.eq_ignore_ascii_case("localhost") {
            return Ok(format!("127.0.0.1:{port}"));
        }
        return Ok(format!("{host}:{port}"));
    }
    Err(format!(
        "invalid --bind {bind:?} (want host:port; default {DEFAULT_BIND})"
    ))
}

fn bind_is_unspecified(bind: &str) -> bool {
    bind.parse::<SocketAddr>()
        .map(|addr| addr.ip().is_unspecified())
        .unwrap_or(false)
}

fn is_loopback_host(bind: &str) -> bool {
    bind.eq_ignore_ascii_case("localhost")
        || bind == "127.0.0.1"
        || bind == "::1"
        || bind == "[::1]"
}

fn loopback_with_default_port(bind: &str) -> String {
    if bind == "::1" || bind == "[::1]" {
        "[::1]:8765".to_string()
    } else {
        DEFAULT_BIND.to_string()
    }
}

fn parse_port_only(bind: &str) -> Option<u16> {
    let digits = bind.strip_prefix(':').unwrap_or(bind);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn split_host_port(bind: &str) -> Option<(&str, u16)> {
    if let Some(rest) = bind.strip_prefix('[') {
        let (host, tail) = rest.split_once("]:")?;
        if host.is_empty() {
            return None;
        }
        return Some((host, tail.parse().ok()?));
    }
    let (host, port) = bind.rsplit_once(':')?;
    if looks_like_dotted_ip(host) {
        return None;
    }
    Some((host, port.parse().ok()?))
}

fn looks_like_dotted_ip(host: &str) -> bool {
    host.contains('.') && host.chars().all(|c| c.is_ascii_digit() || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_strips_query_and_slash() {
        assert_eq!(normalize_path("/health"), "/health");
        assert_eq!(normalize_path("/optimize/?x=1"), "/optimize");
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn json_sniff_and_svg_content_type() {
        assert!(wants_json("application/json; charset=utf-8", b""));
        assert!(!wants_json("image/svg+xml", b"{"));
        assert!(wants_json("", b"  {\"svg\":\"<svg/>\"}"));
        assert!(!wants_json("", b"<svg/>"));
    }

    #[test]
    fn resolve_bind_defaults_and_rewrites_to_loopback() {
        assert_eq!(resolve_bind("").unwrap(), DEFAULT_BIND);
        assert_eq!(resolve_bind("   ").unwrap(), DEFAULT_BIND);
        assert_eq!(resolve_bind("127.0.0.1:8765").unwrap(), DEFAULT_BIND);
        assert_eq!(resolve_bind("localhost").unwrap(), DEFAULT_BIND);
        assert_eq!(resolve_bind("127.0.0.1").unwrap(), DEFAULT_BIND);
        assert_eq!(resolve_bind(":9321").unwrap(), "127.0.0.1:9321");
        assert_eq!(resolve_bind("9321").unwrap(), "127.0.0.1:9321");
        assert_eq!(resolve_bind("*:9321").unwrap(), "127.0.0.1:9321");
        assert_eq!(resolve_bind("localhost:9321").unwrap(), "127.0.0.1:9321");
        let v6 = resolve_bind("[::1]:9321").unwrap();
        let addr: SocketAddr = v6.parse().unwrap();
        assert!(addr.ip().is_loopback());
        assert_eq!(addr.port(), 9321);
    }

    #[test]
    fn resolve_bind_refuses_public_host_without_port() {
        for bad in ["0.0.0.0", "::", "[::]", "192.168.0.1", "not-a-bind", ":"] {
            assert!(resolve_bind(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn resolve_bind_keeps_explicit_container_wildcard() {
        assert_eq!(resolve_bind("0.0.0.0:8080").unwrap(), "0.0.0.0:8080");
        let any6 = resolve_bind("[::]:8080").unwrap();
        let addr: SocketAddr = any6.parse().unwrap();
        assert!(addr.ip().is_unspecified());
        assert_eq!(addr.port(), 8080);
        assert!(bind_is_unspecified("0.0.0.0:8080"));
        assert!(!bind_is_unspecified(DEFAULT_BIND));
    }
}
