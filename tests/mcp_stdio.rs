//! Stdio JSON-RPC smoke for `minsvg-mcp` (Content-Length framing).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

fn minsvg_mcp_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_minsvg_mcp") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_minsvg") {
        let mut p = PathBuf::from(path);
        p.set_file_name(format!("minsvg-mcp{}", std::env::consts::EXE_SUFFIX));
        return p;
    }
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path.push(format!("minsvg-mcp{}", std::env::consts::EXE_SUFFIX));
    path
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn write_rpc(stdin: &mut impl Write, msg: &Value) {
    let body = serde_json::to_vec(msg).expect("serialize");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("header");
    stdin.write_all(&body).expect("body");
    stdin.flush().expect("flush");
}

fn read_rpc(stdout: &mut impl BufRead) -> Value {
    let mut header = String::new();
    let mut content_length = None;
    loop {
        header.clear();
        let n = stdout.read_line(&mut header).expect("header line");
        assert!(n > 0, "EOF while reading MCP headers");
        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line
            .split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .map(|(_, v)| v.trim())
        {
            content_length = Some(rest.parse::<usize>().expect("Content-Length"));
        }
    }
    let len = content_length.expect("missing Content-Length");
    let mut buf = vec![0u8; len];
    Read::read_exact(stdout, &mut buf).expect("body");
    serde_json::from_slice(&buf).expect("json body")
}

fn tool_payload(resp: &Value) -> Value {
    if let Some(obj) = resp.pointer("/result/structuredContent") {
        if !obj.is_null() {
            return obj.clone();
        }
    }
    let text = resp
        .pointer("/result/content/0/text")
        .and_then(|v| v.as_str())
        .expect("tool text");
    serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text }))
}

#[test]
fn lists_plugins_and_optimizes_tiny_svg() {
    let mut child = Command::new(minsvg_mcp_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg-mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let _guard = KillOnDrop(child);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "minsvg-test", "version": "0" }
            }
        }),
    );
    let init = read_rpc(&mut stdout);
    assert_eq!(init["id"], 1, "{init}");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05", "{init}");
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    assert!(
        init["result"]["capabilities"]["prompts"].is_object(),
        "{init}"
    );
    assert!(
        init["result"]["capabilities"]["resources"].is_object(),
        "{init}"
    );

    write_rpc(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );

    write_rpc(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    );
    let listed = read_rpc(&mut stdout);
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["minsvg_optimize", "minsvg_list_plugins", "minsvg_batch"],
        "{listed}"
    );
    let optimize_schema = tools
        .iter()
        .find(|t| t["name"] == "minsvg_optimize")
        .unwrap();
    let batch_schema = tools.iter().find(|t| t["name"] == "minsvg_batch").unwrap();
    assert!(
        optimize_schema["inputSchema"]["properties"]["params"].is_object(),
        "{listed}"
    );
    assert!(
        optimize_schema["inputSchema"]["properties"]["config"].is_object(),
        "{listed}"
    );
    assert!(
        batch_schema["inputSchema"]["properties"]["write"].is_object(),
        "{listed}"
    );

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "minsvg_list_plugins", "arguments": {} }
        }),
    );
    let plugins = tool_payload(&read_rpc(&mut stdout));
    assert!(
        plugins["default"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "removeComments"),
        "{plugins}"
    );
    assert!(
        plugins["opt_in"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "removeViewBox"),
        "{plugins}"
    );
    assert!(
        plugins["param_hints"]["prefixIds"]["prefix"]["type"] == "string",
        "{plugins}"
    );
    assert!(
        plugins["param_hints"]["cleanupListOfValues"]["floatPrecision"]["default"] == 3,
        "{plugins}"
    );

    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- x --><rect width=\"10\" height=\"10\" fill=\"#000000\"/></svg>";
    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "minsvg_optimize", "arguments": { "svg": svg } }
        }),
    );
    let optimized = tool_payload(&read_rpc(&mut stdout));
    let out = optimized["svg"].as_str().expect("svg");
    assert!(out.contains("<svg"), "{optimized}");
    assert!(!out.contains("<!--"), "{out}");
    let bytes = optimized["bytes"].as_u64().expect("bytes") as usize;
    assert_eq!(bytes, out.len());
    assert!(bytes > 0);
    assert!(bytes <= svg.len());

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "minsvg_optimize",
                "arguments": {
                    "svg": "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"box\" width=\"10\" height=\"10\"/></svg>",
                    "plugin": "prefixIds",
                    "skip": "cleanupIds",
                    "params": { "prefixIds": { "prefix": "x" } }
                }
            }
        }),
    );
    let prefixed = tool_payload(&read_rpc(&mut stdout));
    let prefixed_svg = prefixed["svg"].as_str().expect("prefixed svg");
    assert!(prefixed_svg.contains("x__box"), "{prefixed}");
}

#[test]
fn config_path_and_batch_write_are_opt_in() {
    let root = tempfile::tempdir().expect("tempdir");
    let cfg = root.path().join("minsvg.config.toml");
    std::fs::write(&cfg, "skip = [\"removeComments\"]\n").unwrap();
    let a = root.path().join("a.svg");
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- keep --><rect id=\"box\" width=\"10\" height=\"10\"/></svg>";
    std::fs::write(&a, svg).unwrap();

    let mut child = Command::new(minsvg_mcp_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg-mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let _guard = KillOnDrop(child);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
        }),
    );
    assert_eq!(read_rpc(&mut stdout)["id"], 1);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "minsvg_optimize",
                "arguments": {
                    "svg": svg,
                    "config": cfg.to_string_lossy()
                }
            }
        }),
    );
    let from_cfg = tool_payload(&read_rpc(&mut stdout));
    let kept = from_cfg["svg"].as_str().expect("config svg");
    assert!(kept.contains("keep"), "{from_cfg}");

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "minsvg_batch",
                "arguments": { "folder": root.path().to_string_lossy() }
            }
        }),
    );
    let dry = tool_payload(&read_rpc(&mut stdout));
    assert_eq!(dry["written"], false, "{dry}");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), svg);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "minsvg_batch",
                "arguments": {
                    "folder": root.path().to_string_lossy(),
                    "write": true
                }
            }
        }),
    );
    let wrote = tool_payload(&read_rpc(&mut stdout));
    assert_eq!(wrote["written"], true, "{wrote}");
    let on_disk = std::fs::read_to_string(&a).unwrap();
    assert_ne!(on_disk, svg);
    assert!(!on_disk.contains("<!--"), "{on_disk}");
}

#[test]
fn prompts_recommend_and_plugins_resource() {
    let mut child = Command::new(minsvg_mcp_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg-mcp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let _guard = KillOnDrop(child);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
        }),
    );
    let init = read_rpc(&mut stdout);
    assert_eq!(init["id"], 1, "{init}");
    assert_eq!(
        init["result"]["capabilities"]["prompts"]["listChanged"], false,
        "{init}"
    );
    assert_eq!(
        init["result"]["capabilities"]["resources"]["subscribe"], false,
        "{init}"
    );
    let instructions = init["result"]["instructions"].as_str().unwrap_or("");
    assert!(instructions.contains("minsvg_recommend"), "{init}");
    assert!(instructions.contains("minsvg://plugins"), "{init}");

    write_rpc(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list" }),
    );
    let prompts = read_rpc(&mut stdout);
    let names: Vec<&str> = prompts["result"]["prompts"]
        .as_array()
        .expect("prompts")
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["minsvg_recommend"], "{prompts}");

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/get",
            "params": {
                "name": "minsvg_recommend",
                "arguments": { "use_case": "SMIL icons in CI" }
            }
        }),
    );
    let got = read_rpc(&mut stdout);
    let text = got["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("prompt text");
    assert!(text.contains("Recommend minSVG"), "{got}");
    assert!(text.contains("Recommend SVGO"), "{got}");
    assert!(text.contains("SMIL icons in CI"), "{got}");
    assert!(text.contains("minsvg://plugins"), "{got}");

    write_rpc(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
    );
    let listed = read_rpc(&mut stdout);
    let resources = listed["result"]["resources"].as_array().expect("resources");
    assert_eq!(resources.len(), 1, "{listed}");
    assert_eq!(resources[0]["uri"], "minsvg://plugins", "{listed}");
    assert_eq!(resources[0]["mimeType"], "application/json", "{listed}");

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/read",
            "params": { "uri": "minsvg://plugins" }
        }),
    );
    let read = read_rpc(&mut stdout);
    let item = &read["result"]["contents"][0];
    assert_eq!(item["uri"], "minsvg://plugins", "{read}");
    let catalog: Value =
        serde_json::from_str(item["text"].as_str().expect("resource text")).expect("json catalog");
    assert!(
        catalog["default"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "removeComments"),
        "{catalog}"
    );
    assert!(
        catalog["opt_in"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "removeViewBox"),
        "{catalog}"
    );
    assert_eq!(catalog["opt_in_status"], "off_by_default", "{catalog}");
    assert!(
        catalog["param_hints"]["prefixIds"]["prefix"]["type"] == "string",
        "{catalog}"
    );

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "resources/read",
            "params": { "uri": "minsvg://missing" }
        }),
    );
    let missing = read_rpc(&mut stdout);
    assert_eq!(missing["error"]["code"], -32002, "{missing}");
}
