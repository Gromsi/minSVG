//! Wave 2 parity: JS-shaped JSON / YAML config, CLI `--plugin`, MCP catalog.
//!
//! Opt-in plugins stay **off** on `Config::default()`. `removeViewBox` runs
//! only via `--plugin` / a config `plugins` entry (same meaning as `--plugin`).

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use minsvg::{
    apply_plugin_cli, load_config_path, merge_config, optimize_str, optimize_str_with,
    parse_config_json, parse_config_yaml, parse_plugin_spec, CliOverrides, Config,
    DEFAULT_PLUGIN_NAMES, OPTIN_PLUGIN_NAMES,
};
use serde_json::{json, Value};

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
    "</svg>",
);

const MARKED: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
    r##"<defs><circle id="dot" r="2"/></defs>"##,
    r##"<use href="#dot"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn write_named(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn assert_viewbox_kept(svg: &str) {
    assert!(svg.contains("viewBox=\"0 0 24 24\""), "{svg}");
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
}

fn assert_viewbox_dropped(svg: &str) {
    assert!(!svg.contains("viewBox"), "{svg}");
    assert!(svg.contains("width=\"24\""), "{svg}");
    assert!(svg.contains("height=\"24\""), "{svg}");
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
}

#[test]
fn optin_catalog_stays_off_the_default_set() {
    assert!(OPTIN_PLUGIN_NAMES.contains(&"removeViewBox"));
    assert!(OPTIN_PLUGIN_NAMES.contains(&"prefixIds"));
    for name in OPTIN_PLUGIN_NAMES {
        assert!(
            !DEFAULT_PLUGIN_NAMES.contains(name),
            "{name} must stay default OFF"
        );
    }
}

#[test]
fn default_optimize_keeps_viewbox() {
    let cfg = Config::default();
    assert!(cfg.enable_plugins.is_empty());
    assert!(!cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str(ICON).expect("optimize");
    assert_viewbox_kept(&out.svg);
    assert!(!out.plugins_run.iter().any(|n| *n == "removeViewBox"));
}

#[test]
fn enable_remove_viewbox_drops_only_when_requested() {
    let mut cfg = Config::default();
    cfg.enable_plugins.push("removeViewBox".into());
    let out = optimize_str_with(ICON, &cfg).expect("optimize --plugin removeViewBox");
    assert_viewbox_dropped(&out.svg);
    assert!(out.plugins_run.iter().any(|n| *n == "removeViewBox"));
}

#[test]
fn js_shaped_json_without_plugins_keeps_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_named(
        dir.path(),
        "svgo.config.json",
        r#"{ "multipass": false, "floatPrecision": 3 }"#,
    );
    let cfg = load_config_path(&path).unwrap();
    assert!(!cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str_with(ICON, &cfg).expect("json config");
    assert_viewbox_kept(&out.svg);
}

#[test]
fn js_shaped_json_plugins_enable_remove_viewbox() {
    let file = parse_config_json(
        r#"{ "plugins": [{ "name": "removeViewBox" }, { "name": "removeComments", "active": false }] }"#,
    )
    .unwrap();
    let cfg = merge_config(Some(&file), &CliOverrides::default());
    assert!(cfg.plugin_enabled("removeViewBox"));
    assert!(cfg.skip_plugins.iter().any(|s| s == "removeComments"));
    let out = optimize_str_with(ICON, &cfg).expect("json plugins");
    assert_viewbox_dropped(&out.svg);
}

#[test]
fn js_shaped_json_active_false_does_not_drop_viewbox() {
    let file =
        parse_config_json(r#"{ "plugins": [{ "name": "removeViewBox", "active": false }] }"#)
            .unwrap();
    let cfg = merge_config(Some(&file), &CliOverrides::default());
    assert!(!cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str_with(ICON, &cfg).expect("json active:false");
    assert_viewbox_kept(&out.svg);
}

#[test]
fn yaml_config_without_plugins_keeps_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_named(
        dir.path(),
        "minsvg.config.yaml",
        "multipass: false\nfloatPrecision: 3\n",
    );
    let cfg = load_config_path(&path).unwrap();
    assert!(!cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str_with(ICON, &cfg).expect("yaml no plugins");
    assert_viewbox_kept(&out.svg);
}

#[test]
fn yaml_config_plugins_enable_remove_viewbox() {
    let file = parse_config_yaml(
        r#"
plugins:
  - name: removeViewBox
js2svg:
  pretty: false
"#,
    )
    .unwrap();
    let cfg = merge_config(Some(&file), &CliOverrides::default());
    assert!(cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str_with(ICON, &cfg).expect("yaml plugins");
    assert_viewbox_dropped(&out.svg);
}

#[test]
fn yaml_and_json_prefix_ids_params() {
    let json = parse_config_json(
        r#"{
          "plugins": [
            { "name": "cleanupIds", "active": false },
            { "name": "prefixIds", "params": { "prefix": "x", "delim": "_" } }
          ]
        }"#,
    )
    .unwrap();
    let yaml = parse_config_yaml(
        r#"
plugins:
  - name: cleanupIds
    active: false
  - name: prefixIds
    params:
      prefix: x
      delim: "_"
"#,
    )
    .unwrap();
    for file in [&json, &yaml] {
        let cfg = merge_config(Some(file), &CliOverrides::default());
        assert!(cfg.plugin_enabled("prefixIds"));
        assert_eq!(cfg.plugin_param("prefixIds").unwrap()["prefix"], "x");
        let out = optimize_str_with(MARKED, &cfg).expect("prefixIds");
        assert!(
            out.svg.contains("id=\"x_dot\"") || out.svg.contains("x_dot"),
            "{}",
            out.svg
        );
        assert!(out.plugins_run.iter().any(|n| *n == "prefixIds"));
    }
}

#[test]
fn js_plugin_spec_and_param_without_plugin() {
    let mut enabled = Config::default();
    let spec = parse_plugin_spec(r#"prefixIds:{"prefix":"x"}"#).unwrap();
    apply_plugin_cli(&mut enabled, &[spec], &[]).unwrap();
    assert!(enabled.plugin_enabled("prefixIds"));
    assert_eq!(enabled.plugin_param("prefixIds").unwrap()["prefix"], "x");

    let mut params_only = Config::default();
    apply_plugin_cli(
        &mut params_only,
        &[],
        &[r#"removeViewBox={"fake":true}"#.into()],
    )
    .unwrap();
    assert!(
        !params_only.plugin_enabled("removeViewBox"),
        "--param must not enable an opt-in"
    );
    let out = optimize_str_with(ICON, &params_only).expect("param-only");
    assert_viewbox_kept(&out.svg);
}

#[test]
fn cli_default_keeps_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg"])
        .status()
        .expect("spawn minsvg");
    assert!(status.success(), "minsvg default failed: {status}");
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert_viewbox_kept(&out);
}

#[test]
fn cli_plugin_remove_viewbox_drops_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg", "--plugin", "removeViewBox"])
        .status()
        .expect("spawn minsvg --plugin removeViewBox");
    assert!(
        status.success(),
        "minsvg --plugin removeViewBox failed: {status}"
    );
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert_viewbox_dropped(&out);
}

#[test]
fn cli_yaml_and_json_config_enable_remove_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    write_named(
        dir.path(),
        "wave2.yaml",
        "plugins:\n  - name: removeViewBox\n",
    );
    write_named(
        dir.path(),
        "wave2.json",
        r#"{"plugins":[{"name":"removeViewBox"}]}"#,
    );

    for config in ["wave2.yaml", "wave2.json"] {
        let out_name = format!("out-{config}.svg");
        let status = minsvg()
            .current_dir(dir.path())
            .args(["in.svg", "-o", &out_name, "--config", config])
            .status()
            .expect("spawn minsvg --config");
        assert!(
            status.success(),
            "minsvg --config {config} failed: {status}"
        );
        let out = fs::read_to_string(dir.path().join(&out_name)).unwrap();
        assert_viewbox_dropped(&out);
    }
}

#[test]
fn mcp_catalog_lists_remove_viewbox_as_opt_in() {
    let output = minsvg()
        .args(["plugins", "--json"])
        .output()
        .expect("minsvg plugins --json");
    assert!(output.status.success());
    let payload = String::from_utf8_lossy(&output.stdout);
    assert!(payload.contains("\"optin\":["), "{payload}");
    assert!(payload.contains("\"removeViewBox\""), "{payload}");
    let parsed: Value = serde_json::from_str(&payload).expect("plugins json");
    let optin = parsed["optin"].as_array().expect("optin array");
    let wired = parsed["wired"].as_array().expect("wired array");
    assert!(optin.iter().any(|v| v == "removeViewBox"));
    assert!(!wired.iter().any(|v| v == "removeViewBox"));
}

#[test]
fn mcp_stdio_optimize_keeps_viewbox_unless_plugin() {
    let Some(bin) = option_env!("CARGO_BIN_EXE_minsvg_mcp") else {
        return;
    };
    let mut child = Command::new(bin)
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
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05", "{init}");

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "minsvg_list_plugins", "arguments": {} }
        }),
    );
    let listed = tool_payload(&read_rpc(&mut stdout));
    assert_eq!(listed["opt_in_status"], "off_by_default");
    assert!(listed["opt_in"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "removeViewBox"));
    assert!(!listed["default"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "removeViewBox"));

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "minsvg_optimize", "arguments": { "svg": ICON } }
        }),
    );
    let defaulted = tool_payload(&read_rpc(&mut stdout));
    let kept = defaulted["svg"].as_str().expect("svg");
    assert_viewbox_kept(kept);

    write_rpc(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "minsvg_optimize",
                "arguments": { "svg": ICON, "plugin": "removeViewBox" }
            }
        }),
    );
    let enabled = tool_payload(&read_rpc(&mut stdout));
    let dropped = enabled["svg"].as_str().expect("svg");
    assert_viewbox_dropped(dropped);
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
