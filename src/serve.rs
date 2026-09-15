//! Self-hosted optimize HTTP. You bind it; we do not host it.
//!
//! Built only with `--features serve` (tiny_http + serde_json, not axum).

use minsvg::optimize_str;
use std::io::Read;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: u64 = 32 * 1024 * 1024;
const HEALTH_JSON: &str = "{\"ok\":true}";

pub fn run(bind: &str) -> Result<(), String> {
    let server = Server::http(bind).map_err(|e| format!("bind {bind}: {e}"))?;
    let shown = match server.server_addr().to_ip() {
        Some(addr) => format!("http://{addr}"),
        None => format!("http://{bind}"),
    };
    eprintln!("minsvg serve listening on {shown} (you run this; no auth)");
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
}
