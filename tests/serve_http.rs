//! Loopback smoke for `minsvg serve`. No public bind, no fat fixtures.
//! Default / port-only `--bind` must stay on 127.0.0.1 (see `serve::resolve_bind`).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn wait_for_listen(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("minsvg serve did not listen on 127.0.0.1:{port}");
}

fn raw_http(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read");
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn health_and_optimize_on_ephemeral_loopback() {
    let port = free_loopback_port();
    let bind = format!("127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_minsvg"))
        .args(["serve", "--bind", &bind])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg serve");
    let _guard = KillOnDrop(child);
    wait_for_listen(port);

    let health = raw_http(
        port,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(health.contains("200"), "{health}");
    assert!(health.contains("{\"ok\":true}"), "{health}");

    // Single-quoted attrs so the JSON wrapper stays valid without serde in this crate.
    let svg = "<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>";
    let json_body = format!(r#"{{"svg":"{svg}"}}"#);
    let json_req = format!(
        "POST /optimize HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json_body}",
        json_body.len()
    );
    let json_res = raw_http(port, &json_req);
    assert!(json_res.contains("200"), "{json_res}");
    assert!(json_res.contains("\"data\""), "{json_res}");
    assert!(json_res.contains("<svg"), "{json_res}");

    let raw_req = format!(
        "POST /optimize HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{svg}",
        svg.len()
    );
    let raw_res = raw_http(port, &raw_req);
    assert!(raw_res.contains("200"), "{raw_res}");
    assert!(raw_res.contains("image/svg+xml"), "{raw_res}");
    assert!(raw_res.contains("<svg"), "{raw_res}");
}

#[test]
fn port_only_bind_listens_on_loopback() {
    let port = free_loopback_port();
    let bind = format!(":{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_minsvg"))
        .args(["serve", "--bind", &bind])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg serve");
    let _guard = KillOnDrop(child);
    wait_for_listen(port);

    let health = raw_http(
        port,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(health.contains("200"), "{health}");
    assert!(health.contains("{\"ok\":true}"), "{health}");
}

#[test]
fn serve_refuses_public_bind_without_port() {
    let output = Command::new(env!("CARGO_BIN_EXE_minsvg"))
        .args(["serve", "--bind", "0.0.0.0"])
        .output()
        .expect("run minsvg serve");
    assert!(!output.status.success(), "{output:?}");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("invalid --bind") && err.contains("127.0.0.1:8765"),
        "{err}"
    );
}
