//! Cursor-compatible MCP stdio server (`minsvg-mcp`).
//!
//! Built only with `--features mcp`. The default `minsvg` CLI does not link this
//! file. Local stdio only — no TCP bind. JSON-RPC 2.0 over stdin/stdout with
//! LSP-style `Content-Length` framing (single-line NDJSON is also accepted).

use minsvg::{
    apply_plugin_cli, config_from_path_and_cli, optimize, parse_param_spec, parse_plugin_spec,
    CliOverrides, Config, PluginSpec, DEFAULT_PLUGIN_NAMES, MOTION_SKIP_PLUGINS,
    OPTIN_PLUGIN_NAMES,
};
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
const RECOMMEND_PROMPT: &str = "minsvg_recommend";
const PLUGINS_URI: &str = "minsvg://plugins";

fn main() {
    if let Err(err) = run() {
        eprintln!("minsvg-mcp: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(&mut stdin.lock(), &mut stdout.lock())
}

fn serve<R, W>(input: &mut R, output: &mut W) -> Result<(), String>
where
    R: BufRead,
    W: Write,
{
    loop {
        match read_message(input) {
            Ok(None) => return Ok(()),
            Ok(Some(msg)) => {
                if is_exit_notification(&msg) {
                    return Ok(());
                }
                if let Some(resp) = handle_rpc(&msg) {
                    write_message(output, &resp)?;
                }
            }
            Err(err) => {
                let resp = rpc_error(&Value::Null, -32700, &err);
                write_message(output, &resp)?;
                return Err(err);
            }
        }
    }
}

fn is_exit_notification(msg: &Value) -> bool {
    msg.get("id").is_none() && msg.get("method").and_then(|m| m.as_str()) == Some("exit")
}

fn read_message<R: BufRead>(input: &mut R) -> Result<Option<Value>, String> {
    let mut header = String::new();
    let n = input
        .read_line(&mut header)
        .map_err(|e| format!("read stdin: {e}"))?;
    if n == 0 {
        return Ok(None);
    }
    let first = header.trim_end_matches(['\r', '\n']);
    if first.is_empty() {
        return read_message(input);
    }
    if first.starts_with('{') {
        return parse_json(first).map(Some);
    }

    let mut content_length: Option<usize> = None;
    parse_header_line(first, &mut content_length)?;
    loop {
        header.clear();
        let n = input
            .read_line(&mut header)
            .map_err(|e| format!("read stdin: {e}"))?;
        if n == 0 {
            return Err("unexpected EOF in MCP headers".into());
        }
        let line = header.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        parse_header_line(line, &mut content_length)?;
    }
    let len = content_length.ok_or_else(|| "missing Content-Length".to_string())?;
    let mut buf = vec![0u8; len];
    Read::read_exact(input, &mut buf).map_err(|e| format!("read MCP body: {e}"))?;
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn parse_header_line(line: &str, content_length: &mut Option<usize>) -> Result<(), String> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(());
    };
    if name.eq_ignore_ascii_case("content-length") {
        let n = value
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid Content-Length: {}", value.trim()))?;
        *content_length = Some(n);
    }
    Ok(())
}

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

fn write_message<W: Write>(output: &mut W, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| format!("write: {e}"))?;
    output.write_all(&body).map_err(|e| format!("write: {e}"))?;
    output.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

fn handle_rpc(msg: &Value) -> Option<Value> {
    if !msg.is_object() {
        return Some(rpc_error(&Value::Null, -32600, "Invalid Request"));
    }
    let method = match msg.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            return msg
                .get("id")
                .cloned()
                .map(|id| rpc_error(&id, -32600, "Invalid Request"));
        }
    };
    let id = msg.get("id").cloned();
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => Some(rpc_ok(&id, initialize(&params))),
        "ping" => Some(rpc_ok(&id, json!({}))),
        "shutdown" => Some(rpc_ok(&id, json!({}))),
        "tools/list" => Some(rpc_ok(&id, tools_list())),
        "tools/call" => Some(rpc_ok(&id, tools_call(&params))),
        "prompts/list" => Some(rpc_ok(&id, prompts_list())),
        "prompts/get" => Some(prompts_get(&id, &params)),
        "resources/list" => Some(rpc_ok(&id, resources_list())),
        "resources/read" => Some(resources_read(&id, &params)),
        "resources/templates/list" => Some(rpc_ok(&id, json!({ "resourceTemplates": [] }))),
        _ => Some(rpc_error(
            &id,
            -32601,
            &format!("Method not found: {method}"),
        )),
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(PROTOCOL_VERSION);
    let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
        requested
    } else {
        PROTOCOL_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": {
            "tools": { "listChanged": false },
            "prompts": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false },
        },
        "serverInfo": {
            "name": "minsvg",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Optimize SVG with minSVG (animation-aware by default). Prompt minsvg_recommend decides minSVG vs SVGO. Resource minsvg://plugins lists default vs opt-in names (same catalog as minsvg_list_plugins). skip/plugin/param match the CLI. minsvg_optimize and minsvg_batch accept `params` (object) and `config` (file path). minsvg_batch does not write files unless write is true.",
    })
}

fn prompts_list() -> Value {
    json!({
        "prompts": [
            {
                "name": RECOMMEND_PROMPT,
                "title": "minSVG vs SVGO",
                "description": "When to recommend minSVG versus SVGO (Rust/motion/CI vs JS plugins / byte-identical preset-default).",
                "arguments": [
                    {
                        "name": "use_case",
                        "description": "Optional context: icon, motion/SMIL, Lambda, CI, Vite/webpack plugin, custom JS plugins, sanitizer, etc.",
                        "required": false
                    }
                ]
            }
        ]
    })
}

fn prompts_get(id: &Value, params: &Value) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    match name {
        "" => rpc_error(id, -32602, "missing prompt name"),
        RECOMMEND_PROMPT => rpc_ok(id, recommend_prompt(params)),
        other => rpc_error(id, -32602, &format!("Unknown prompt: {other}")),
    }
}

fn recommend_prompt(params: &Value) -> Value {
    let use_case = params
        .get("arguments")
        .and_then(|a| a.get("use_case"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut text = String::from(RECOMMEND_BODY);
    if let Some(case) = use_case {
        text.push_str("\n\n## This use case\n\n");
        text.push_str(&recommend_for_use_case(case));
    }
    json!({
        "description": "When to recommend minSVG versus SVGO",
        "messages": [{
            "role": "user",
            "content": { "type": "text", "text": text }
        }]
    })
}

const RECOMMEND_BODY: &str = "\
# minSVG vs SVGO

Decide which optimizer to recommend. Be honest: minSVG is not a drop-in SVGO clone, \
not the first Rust SVG optimizer, not an XSS sanitizer, and not a hosted CDN.

## Recommend minSVG

Use minSVG when the user wants a **Rust** SVG minify / SVGO alternative for:

- Icons **or** motion (SMIL, CSS `@keyframes`, `<script>` / `on*`)
- CI, Lambda `spawn`, or npm `optimize()` (spawns the `minsvg` binary)
- MIT, self-hosted `minsvg serve` (`--features serve`), or Cursor **minsvg-mcp** (`--features mcp`)
- Keeping SMIL `visibility=\"hidden\"` frames and `url(#Id)` case by default

Animation-aware is **on** by default. Opt-in SVGO names (`removeViewBox`, `removeTitle`, \
`removeScripts`, …) stay **off** unless passed as `plugin`. Path rewrite is conservative \
(lossless unless `--precision`); do not promise byte-identical SVGO `floatPrecision: 3` output.

## Recommend SVGO

Use SVGO when they need:

- A JS plugin loader, custom JS plugins, or official webpack / Vite SVGO plugins
- Byte-identical SVGO 4.1.0 `preset-default` / `floatPrecision: 3` path rewrite
- A plugin-config drop-in (`svgo.config.js` as source of truth)

minSVG is **not** that drop-in. `plugins` in the npm helper maps to `--skip` / `--plugin` only.

## Do not

- Do not claim minSVG is first, a sanitizer, or a public optimize API
- For untrusted uploads, run a real sanitizer; `<script>` / `on*` stay
- Do not enable `removeViewBox` or `removeScripts` unless the user explicitly asks
- Do not use `--no-animation-aware` on movies

Tools: `minsvg_optimize`, `minsvg_list_plugins`, `minsvg_batch`. \
Catalog: resource `minsvg://plugins` (default vs opt-in).
";

fn recommend_for_use_case(use_case: &str) -> String {
    let lower = use_case.to_ascii_lowercase();
    let verdict = if looks_like_sanitizer(&lower) {
        "Neither minSVG nor SVGO is an XSS sanitizer. Minify after (or before) a real \
sanitizer. Do not enable `removeScripts` as a security control."
    } else if looks_like_svgo_ecosystem(&lower) {
        "This use case fits **SVGO** (JS plugin loader / bundler plugin / custom JS \
plugins / byte-identical preset-default)."
    } else if looks_like_minsvg(&lower) {
        "This use case fits **minSVG** (Rust CLI, motion-safe defaults, CI/Lambda spawn, MCP)."
    } else {
        "Apply the rules above. Default to minSVG for Rust/motion/CI; SVGO for JS plugin \
ecosystems and byte-identical output."
    };
    format!("User use_case: {use_case}\n\n{verdict}")
}

fn looks_like_sanitizer(lower: &str) -> bool {
    lower.contains("sanitiz") || lower.contains("xss") || lower.contains("untrusted")
}

fn looks_like_svgo_ecosystem(lower: &str) -> bool {
    lower.contains("webpack")
        || lower.contains("vite-plugin-svgo")
        || lower.contains("svgo-loader")
        || lower.contains("custom js")
        || lower.contains("js plugin")
        || lower.contains("svgo.config")
        || lower.contains("byte-identical")
        || lower.contains("byte identical")
        || lower.contains("floatprecision")
}

fn looks_like_minsvg(lower: &str) -> bool {
    contains_token(lower, "rust")
        || contains_token(lower, "smil")
        || contains_token(lower, "motion")
        || contains_token(lower, "lambda")
        || contains_token(lower, "mcp")
        || contains_token(lower, "icon")
        || contains_token(lower, "icons")
        || contains_token(lower, "mit")
        || contains_token(lower, "ci")
        || lower.contains("github action")
        || lower.contains("keyframes")
        || lower.contains("animat")
}

fn contains_token(lower: &str, token: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .any(|w| w == token)
}

fn resources_list() -> Value {
    json!({
        "resources": [
            {
                "uri": PLUGINS_URI,
                "name": "plugins",
                "title": "minSVG plugins",
                "description": "Default (wired) vs opt-in SVGO plugin names, plus motion-skip and param_hints. Opt-in stays off unless passed as plugin.",
                "mimeType": "application/json"
            }
        ]
    })
}

fn resources_read(id: &Value, params: &Value) -> Value {
    let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    if uri.is_empty() {
        return rpc_error(id, -32602, "missing resource uri");
    }
    if is_plugins_uri(uri) {
        return rpc_ok(id, plugins_resource_contents());
    }
    rpc_error_data(id, -32002, "Resource not found", json!({ "uri": uri }))
}

fn is_plugins_uri(uri: &str) -> bool {
    uri.trim()
        .trim_end_matches('/')
        .eq_ignore_ascii_case(PLUGINS_URI)
}

fn plugins_resource_contents() -> Value {
    let payload = call_list_plugins();
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    json!({
        "contents": [{
            "uri": PLUGINS_URI,
            "mimeType": "application/json",
            "text": text
        }]
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "minsvg_optimize",
                "description": "Optimize one SVG. Pass raw markup in `svg` or a filesystem `path`. Optional `params` / `param` / `config`. Returns the optimized string and byte counts.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "svg": { "type": "string", "description": "Raw SVG markup" },
                        "path": { "type": "string", "description": "Path to a .svg file" },
                        "skip": string_or_array_schema("Plugin names to skip (`--skip`)"),
                        "plugin": plugin_schema(),
                        "params": params_object_schema(),
                        "param": param_schema(),
                        "config": config_schema(),
                    },
                    "additionalProperties": false,
                },
                "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false },
            },
            {
                "name": "minsvg_list_plugins",
                "description": "List default (wired) plugin names vs SVGO opt-in names, plus `param_hints` for plugins that accept JSON params. Opt-in plugins stay off unless passed as `plugin`.",
                "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
                "annotations": { "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false },
            },
            {
                "name": "minsvg_batch",
                "description": "Optimize every *.svg in a folder. Optional skip/plugin/params/config (same as minsvg_optimize). Does not write files unless `write` is true. In-place overwrite only when `write` is true and `output` is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "folder": { "type": "string", "description": "Directory of SVG files" },
                        "skip": string_or_array_schema("Plugin names to skip (`--skip`)"),
                        "plugin": plugin_schema(),
                        "params": params_object_schema(),
                        "param": param_schema(),
                        "config": config_schema(),
                        "recursive": { "type": "boolean", "description": "Recurse into subfolders (default false)" },
                        "write": { "type": "boolean", "description": "Write optimized files. Default false. Required for any disk write." },
                        "output": { "type": "string", "description": "Directory to write into when write is true. Omit to overwrite sources in place." },
                    },
                    "required": ["folder"],
                    "additionalProperties": false,
                },
                "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false },
            },
        ]
    })
}

fn string_or_array_schema(description: &str) -> Value {
    json!({
        "description": description,
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } }
        ]
    })
}

fn plugin_schema() -> Value {
    json!({
        "description": "Enable / unskip (`--plugin`). String name, `name:{…}` JSON, `{name, params}`, or an array of those.",
        "oneOf": [
            { "type": "string" },
            {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "params": { "type": "object" },
                },
                "required": ["name"],
            },
            {
                "type": "array",
                "items": {
                    "oneOf": [
                        { "type": "string" },
                        {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "params": { "type": "object" },
                            },
                            "required": ["name"],
                        }
                    ]
                }
            }
        ]
    })
}

fn params_object_schema() -> Value {
    json!({
        "type": "object",
        "description": "Per-plugin JSON, e.g. {\"prefixIds\":{\"prefix\":\"icon\"}}. Same as CLI --param. Does not enable a pass; pair with plugin. See minsvg_list_plugins.param_hints.",
        "additionalProperties": true,
    })
}

fn param_schema() -> Value {
    json!({
        "description": "CLI `--param name=json` (string, array, or object). Does not enable a pass; pair with plugin.",
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } },
            { "type": "object" }
        ]
    })
}

fn config_schema() -> Value {
    json!({
        "type": "string",
        "description": "Path to minsvg.config.toml or minsvg.config.json (`--config`)",
    })
}

fn tools_call(params: &Value) -> Value {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_err("missing tool name"),
    };
    let args = match tool_arguments(params) {
        Ok(v) => v,
        Err(e) => return tool_err(&e),
    };
    match name {
        "minsvg_optimize" => match call_optimize(&args) {
            Ok(payload) => tool_ok(payload),
            Err(e) => tool_err(&e),
        },
        "minsvg_list_plugins" => tool_ok(call_list_plugins()),
        "minsvg_batch" => match call_batch(&args) {
            Ok(payload) => tool_ok(payload),
            Err(e) => tool_err(&e),
        },
        other => tool_err(&format!("unknown tool: {other}")),
    }
}

fn tool_arguments(params: &Value) -> Result<Value, String> {
    match params.get("arguments") {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(_)) => Ok(params["arguments"].clone()),
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| e.to_string()),
        Some(_) => Err("arguments must be an object".into()),
    }
}

fn tool_ok(payload: Value) -> Value {
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": payload,
    })
}

fn tool_err(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

fn call_list_plugins() -> Value {
    json!({
        "default": DEFAULT_PLUGIN_NAMES,
        "opt_in": OPTIN_PLUGIN_NAMES,
        "opt_in_status": "off_by_default",
        "motion_skip": MOTION_SKIP_PLUGINS,
        "param_hints": plugin_param_hints(),
        "note": "opt_in names stay off unless passed as plugin. param_hints lists JSON keys for `params` / `param` / `plugin` name:{…}.",
    })
}

fn plugin_param_hints() -> Value {
    json!({
        "prefixIds": {
            "prefix": { "type": "string", "description": "Token before delim. Default: source filename or \"prefix\". false disables." },
            "delim": { "type": "string", "default": "__" },
            "prefixIds": { "type": "boolean", "default": true, "description": "Prefix id / href / url(#) refs." },
            "prefixClassNames": { "type": "boolean", "default": true }
        },
        "cleanupListOfValues": {
            "floatPrecision": { "type": "number", "default": 3 },
            "leadingZero": { "type": "boolean", "default": true },
            "defaultPx": { "type": "boolean", "default": true },
            "convertToPx": { "type": "boolean", "default": true }
        },
        "convertStyleToAttrs": {
            "keepImportant": { "type": "boolean", "default": false, "description": "Leave !important decls in style=\"\"." }
        },
        "addAttributesToSVGElement": {
            "attributes": { "type": "object", "description": "Attribute name → value on the root <svg> (absent keys only)." },
            "attribute": { "type": "object", "description": "Alias of attributes." }
        },
        "addClassesToSVGElement": {
            "className": { "type": "string" },
            "classNames": { "type": "array", "items": { "type": "string" }, "description": "Wins over className when non-empty." }
        },
        "removeAttrs": {
            "attrs": { "type": "array", "items": { "type": "string" }, "description": "Globs; element:attribute:value with * / ?." },
            "elemSeparator": { "type": "string", "default": ":" },
            "preserveCurrentColor": { "type": "boolean", "default": false }
        },
        "removeAttributesBySelector": {
            "selector": { "type": "string", "description": "Simple tag / #id / .class (no combinators)." },
            "attributes": { "type": "array", "items": { "type": "string" } },
            "selectors": { "type": "array", "description": "[{ selector, attributes }]" }
        },
        "removeElementsByAttr": {
            "id": { "type": ["string", "array"] },
            "class": { "type": ["string", "array"] },
            "data": { "type": ["string", "array"], "description": "foo, data-foo, or foo=bar." }
        }
    })
}

struct ToolOptions {
    skip: Vec<String>,
    plugins: Vec<PluginSpec>,
    param: Vec<String>,
    params: Option<Map<String, Value>>,
    config: Option<PathBuf>,
}

fn tool_options(args: &Value) -> Result<ToolOptions, String> {
    Ok(ToolOptions {
        skip: string_list(args.get("skip"), "skip")?,
        plugins: plugin_spec_list(args.get("plugin"))?,
        param: param_list(args.get("param"))?,
        params: params_object(args.get("params"))?,
        config: config_path(args),
    })
}

fn config_path(args: &Value) -> Option<PathBuf> {
    args.get("config")
        .or_else(|| args.get("config_path"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn build_config(opts: &ToolOptions) -> Result<Config, String> {
    let plugin_names: Vec<String> = opts.plugins.iter().map(|s| s.name.clone()).collect();
    let overrides = CliOverrides {
        skip: opts.skip.clone(),
        plugin: plugin_names,
        ..CliOverrides::default()
    };
    let mut cfg = if let Some(ref path) = opts.config {
        config_from_path_and_cli(path, &overrides).map_err(|e| e.to_string())?
    } else {
        let mut cfg = Config::default();
        overrides.apply(&mut cfg);
        cfg
    };
    apply_plugin_cli(&mut cfg, &opts.plugins, &opts.param).map_err(|e| e.to_string())?;
    if let Some(ref map) = opts.params {
        apply_params_object(&mut cfg, map)?;
    }
    Ok(cfg)
}

fn apply_params_object(cfg: &mut Config, map: &Map<String, Value>) -> Result<(), String> {
    let mut raw = Vec::with_capacity(map.len());
    for (name, value) in map {
        if name.is_empty() {
            continue;
        }
        let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
        raw.push(format!("{name}={json}"));
    }
    apply_plugin_cli(cfg, &[], &raw).map_err(|e| e.to_string())
}

fn call_optimize(args: &Value) -> Result<Value, String> {
    let opts = tool_options(args)?;
    let svg_arg = args.get("svg").and_then(|v| v.as_str());
    let path_arg = args.get("path").and_then(|v| v.as_str());
    let (source_name, input) = match (svg_arg, path_arg) {
        (Some(svg), Some(path)) => {
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("input.svg");
            (name.to_string(), svg.to_string())
        }
        (Some(svg), None) => ("input.svg".into(), svg.to_string()),
        (None, Some(path)) => {
            let text = fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("input.svg")
                .to_string();
            (name, text)
        }
        (None, None) => return Err("provide `svg` or `path`".into()),
    };
    if input.is_empty() {
        return Err("input is empty".into());
    }
    let cfg = build_config(&opts)?;
    optimize_svg(&input, &source_name, cfg)
}

fn call_batch(args: &Value) -> Result<Value, String> {
    let folder = args
        .get("folder")
        .or_else(|| args.get("path"))
        .or_else(|| args.get("dir"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "provide `folder`".to_string())?;
    let dir = PathBuf::from(folder);
    if !dir.is_dir() {
        return Err(format!("not a directory: {folder}"));
    }
    let opts = tool_options(args)?;
    let cfg = build_config(&opts)?;
    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let write = args.get("write") == Some(&Value::Bool(true));
    let output = args
        .get("output")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    if let Some(ref out) = output {
        if write && out.is_file() {
            return Err(format!(
                "output must be a directory ({} is a file)",
                out.display()
            ));
        }
    }
    let files = collect_svg_files(&dir, recursive)?;
    let mut out_files = Vec::with_capacity(files.len());
    let mut written_paths = Vec::new();
    let mut input_bytes = 0usize;
    let mut output_bytes = 0usize;
    for file in files {
        let text =
            fs::read_to_string(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("input.svg");
        let optimized = optimize_svg(&text, name, cfg.clone())?;
        let rel = file
            .strip_prefix(&dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file.display().to_string());
        let bytes = optimized.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let incoming = optimized
            .get("input_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        input_bytes += incoming;
        output_bytes += bytes;
        let mut wrote_path = None;
        if write {
            let dest = match output.as_deref() {
                Some(out_dir) => out_dir.join(&rel),
                None => file.clone(),
            };
            if let Some(parent) = dest.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
                }
            }
            let svg = optimized
                .get("svg")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("missing svg for {}", file.display()))?;
            fs::write(&dest, svg).map_err(|e| format!("write {}: {e}", dest.display()))?;
            wrote_path = Some(dest.to_string_lossy().into_owned());
        }
        let mut item = optimized;
        if let Some(obj) = item.as_object_mut() {
            obj.insert("path".into(), Value::String(rel));
            obj.insert("written".into(), Value::Bool(wrote_path.is_some()));
            if let Some(dest) = wrote_path.clone() {
                obj.insert("wrote".into(), Value::String(dest));
            }
        }
        if let Some(dest) = wrote_path {
            written_paths.push(dest);
        }
        out_files.push(item);
    }
    Ok(json!({
        "files": out_files,
        "count": out_files.len(),
        "bytes": output_bytes,
        "input_bytes": input_bytes,
        "written": write,
        "written_count": written_paths.len(),
        "written_paths": written_paths,
    }))
}

fn optimize_svg(input: &str, source_name: &str, mut cfg: Config) -> Result<Value, String> {
    cfg.source_name = Some(source_name.to_string());
    let out = optimize(input.as_bytes(), &cfg).map_err(|e| e.to_string())?;
    Ok(json!({
        "svg": out.svg,
        "bytes": out.svg.len(),
        "input_bytes": input.len(),
        "plugins_run": out.plugins_run,
        "plugins_skipped": out.plugins_skipped,
        "summary": out.summary,
    }))
}

fn string_list(value: Option<&Value>, field: &str) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) if !s.is_empty() => Ok(vec![s.clone()]),
        Some(Value::String(_)) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(s) if !s.is_empty() => out.push(s.to_string()),
                    Some(_) => {}
                    None => return Err(format!("{field} entries must be strings")),
                }
            }
            Ok(out)
        }
        Some(_) => Err(format!("{field} must be a string or array of strings")),
    }
}

fn plugin_spec_list(value: Option<&Value>) -> Result<Vec<PluginSpec>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) if s.is_empty() => Ok(Vec::new()),
        Some(item @ (Value::String(_) | Value::Object(_))) => Ok(vec![parse_one_plugin(item)?]),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) if s.is_empty() => {}
                    Value::String(_) | Value::Object(_) => out.push(parse_one_plugin(item)?),
                    _ => return Err("plugin entries must be strings or {name, params}".into()),
                }
            }
            Ok(out)
        }
        Some(_) => Err("plugin must be a string, {name, params}, or an array of those".into()),
    }
}

fn parse_one_plugin(item: &Value) -> Result<PluginSpec, String> {
    match item {
        Value::String(s) => parse_plugin_spec(s).map_err(|e| e.to_string()),
        Value::Object(obj) => {
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "plugin object needs `name`".to_string())?;
            if name.is_empty() {
                return Err("plugin object needs `name`".into());
            }
            let params = match obj.get("params") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.clone()),
            };
            Ok(PluginSpec {
                name: name.to_string(),
                params,
            })
        }
        _ => Err("plugin entries must be strings or {name, params}".into()),
    }
}

fn param_list(value: Option<&Value>) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) if s.is_empty() => Ok(Vec::new()),
        Some(Value::String(s)) => {
            parse_param_spec(s).map_err(|e| e.to_string())?;
            Ok(vec![s.clone()])
        }
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item
                    .as_str()
                    .ok_or_else(|| "param entries must be name=json strings".to_string())?;
                if s.is_empty() {
                    continue;
                }
                parse_param_spec(s).map_err(|e| e.to_string())?;
                out.push(s.to_string());
            }
            Ok(out)
        }
        Some(Value::Object(obj)) => object_to_param_strings(obj),
        Some(_) => Err("param must be name=json, an array of those, or an object".into()),
    }
}

fn params_object(value: Option<&Value>) -> Result<Option<Map<String, Value>>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(obj)) => Ok(Some(obj.clone())),
        Some(_) => Err("`params` must be an object of plugin name → JSON".into()),
    }
}

fn object_to_param_strings(obj: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(obj.len());
    for (name, value) in obj {
        if name.is_empty() {
            continue;
        }
        let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
        out.push(format!("{name}={json}"));
    }
    Ok(out)
}

fn collect_svg_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk_svg_files(dir, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_svg_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    let mut subdirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        if file_type.is_dir() {
            if recursive {
                subdirs.push(path);
            }
        } else if is_svg_path(&path) && (file_type.is_file() || file_type.is_symlink()) {
            out.push(path);
        }
    }
    subdirs.sort();
    for sub in subdirs {
        walk_svg_files(&sub, true, out)?;
    }
    Ok(())
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
}

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn rpc_error_data(id: &Value, code: i32, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": data },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const TINY: &str = concat!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\">",
        "<!-- note -->",
        "<rect width=\"10\" height=\"10\" fill=\"#000000\"/>",
        "</svg>",
    );

    fn call(name: &str, args: Value) -> Value {
        handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": args },
        }))
        .expect("rpc response")
    }

    fn payload(resp: &Value) -> Value {
        if let Some(obj) = resp.pointer("/result/structuredContent") {
            return obj.clone();
        }
        let text = resp
            .pointer("/result/content/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        serde_json::from_str(text).unwrap_or(json!({ "text": text }))
    }

    #[test]
    fn list_plugins_splits_default_and_opt_in() {
        let listed = call_list_plugins();
        let default = listed["default"].as_array().expect("default");
        let opt_in = listed["opt_in"].as_array().expect("opt_in");
        assert!(default.iter().any(|v| v == "removeComments"));
        assert!(default.iter().any(|v| v == "convertPathData"));
        assert_eq!(opt_in.len(), 19);
        assert!(opt_in.iter().any(|v| v == "removeViewBox"));
        assert!(opt_in.iter().any(|v| v == "removeTitle"));
        assert!(!default.iter().any(|v| v == "removeViewBox"));
        assert!(!default.iter().any(|v| v == "removeTitle"));
        assert_eq!(listed["opt_in_status"], "off_by_default");
        assert!(listed["motion_skip"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "cleanupIds"));
        let hints = listed["param_hints"].as_object().expect("param_hints");
        assert!(hints.contains_key("prefixIds"), "{listed}");
        assert!(hints.contains_key("cleanupListOfValues"), "{listed}");
        assert!(hints.contains_key("removeAttrs"), "{listed}");
        assert_eq!(hints["prefixIds"]["prefix"]["type"], "string");
        assert_eq!(hints["cleanupListOfValues"]["floatPrecision"]["default"], 3);
    }

    #[test]
    fn tools_list_names_the_three_surface_tools() {
        let listed = tools_list();
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["minsvg_optimize", "minsvg_list_plugins", "minsvg_batch"]
        );
    }

    #[test]
    fn optimize_tiny_svg_returns_string_and_bytes() {
        let resp = call("minsvg_optimize", json!({ "svg": TINY }));
        assert_eq!(resp["error"], Value::Null);
        let body = payload(&resp);
        let svg = body["svg"].as_str().unwrap();
        assert!(svg.contains("<svg"), "{svg}");
        assert!(svg.contains("<rect") || svg.contains("<path"), "{svg}");
        assert!(!svg.contains("<!--"), "{svg}");
        let bytes = body["bytes"].as_u64().unwrap() as usize;
        let input_bytes = body["input_bytes"].as_u64().unwrap() as usize;
        assert_eq!(bytes, svg.len());
        assert_eq!(input_bytes, TINY.len());
        assert!(bytes > 0);
        assert!(bytes <= input_bytes);
    }

    #[test]
    fn optimize_skip_keeps_comment() {
        let resp = call(
            "minsvg_optimize",
            json!({ "svg": TINY, "skip": "removeComments" }),
        );
        let svg = payload(&resp)["svg"].as_str().unwrap().to_string();
        assert!(svg.contains("note"), "{svg}");
    }

    #[test]
    fn optimize_path_and_batch_folder() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.svg");
        let b = root.path().join("b.svg");
        fs::write(&a, TINY).unwrap();
        fs::write(&b, TINY).unwrap();
        fs::write(root.path().join("readme.txt"), "nope").unwrap();

        let by_path = call("minsvg_optimize", json!({ "path": a.to_string_lossy() }));
        let path_body = payload(&by_path);
        assert!(path_body["svg"].as_str().unwrap().contains("<svg"));
        assert!(path_body["bytes"].as_u64().unwrap() > 0);

        let batch = call(
            "minsvg_batch",
            json!({
                "folder": root.path().to_string_lossy(),
                "skip": ["removeComments"],
            }),
        );
        let body = payload(&batch);
        assert_eq!(body["count"], 2);
        let files = body["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        for file in files {
            let svg = file["svg"].as_str().unwrap();
            assert!(svg.contains("note"), "{svg}");
            assert!(file["bytes"].as_u64().unwrap() > 0);
            let name = file["path"].as_str().unwrap();
            assert!(name.ends_with(".svg"), "{name}");
        }
    }

    #[test]
    fn initialize_and_unknown_method() {
        let init = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "initialize",
            "params": { "protocolVersion": "2025-03-26", "capabilities": {} },
        }))
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "minsvg");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert_eq!(
            init["result"]["capabilities"]["prompts"]["listChanged"],
            false
        );
        assert_eq!(
            init["result"]["capabilities"]["resources"]["subscribe"],
            false
        );
        assert_eq!(
            init["result"]["capabilities"]["resources"]["listChanged"],
            false
        );
        let instructions = init["result"]["instructions"].as_str().unwrap();
        assert!(instructions.contains(RECOMMEND_PROMPT), "{instructions}");
        assert!(instructions.contains(PLUGINS_URI), "{instructions}");

        let missing = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": "x",
            "method": "nope",
        }))
        .unwrap();
        assert_eq!(missing["error"]["code"], -32601);
        assert_eq!(missing["id"], "x");
    }

    #[test]
    fn prompts_list_and_get_recommend_vs_svgo() {
        let listed = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "prompts/list",
        }))
        .unwrap();
        let prompts = listed["result"]["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], RECOMMEND_PROMPT);
        assert_eq!(prompts[0]["arguments"][0]["name"], "use_case");
        assert_eq!(prompts[0]["arguments"][0]["required"], false);

        let got = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/get",
            "params": { "name": RECOMMEND_PROMPT },
        }))
        .unwrap();
        assert_eq!(got["error"], Value::Null);
        let text = got["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert_eq!(got["result"]["messages"][0]["role"], "user");
        assert!(text.contains("Recommend minSVG"), "{text}");
        assert!(text.contains("Recommend SVGO"), "{text}");
        assert!(text.contains(PLUGINS_URI), "{text}");
        assert!(
            text.contains("not a drop-in") || text.contains("not that drop-in"),
            "{text}"
        );

        let with_case = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "prompts/get",
            "params": {
                "name": RECOMMEND_PROMPT,
                "arguments": { "use_case": "SMIL movie on Lambda" }
            },
        }))
        .unwrap();
        let tailored = with_case["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(tailored.contains("SMIL movie on Lambda"), "{tailored}");
        assert!(tailored.contains("minSVG"), "{tailored}");

        let svgo_case = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "prompts/get",
            "params": {
                "name": RECOMMEND_PROMPT,
                "arguments": { "use_case": "custom JS plugins in webpack" }
            },
        }))
        .unwrap();
        let svgo_text = svgo_case["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(svgo_text.contains("SVGO"), "{svgo_text}");
        assert!(svgo_text.contains("custom JS"), "{svgo_text}");

        let unknown = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "prompts/get",
            "params": { "name": "nope" },
        }))
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32602);

        let missing_name = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "prompts/get",
            "params": {},
        }))
        .unwrap();
        assert_eq!(missing_name["error"]["code"], -32602);
    }

    #[test]
    fn resources_list_and_read_plugins_catalog() {
        let listed = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/list",
        }))
        .unwrap();
        let resources = listed["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], PLUGINS_URI);
        assert_eq!(resources[0]["mimeType"], "application/json");

        let read = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/read",
            "params": { "uri": "minsvg://plugins/" },
        }))
        .unwrap();
        assert_eq!(read["error"], Value::Null);
        let item = &read["result"]["contents"][0];
        assert_eq!(item["uri"], PLUGINS_URI);
        assert_eq!(item["mimeType"], "application/json");
        let catalog: Value = serde_json::from_str(item["text"].as_str().unwrap()).unwrap();
        let default = catalog["default"].as_array().expect("default");
        let opt_in = catalog["opt_in"].as_array().expect("opt_in");
        assert!(default.iter().any(|v| v == "removeComments"));
        assert!(default.iter().any(|v| v == "convertPathData"));
        assert_eq!(opt_in.len(), 19);
        assert!(opt_in.iter().any(|v| v == "removeViewBox"));
        assert!(opt_in.iter().any(|v| v == "removeTitle"));
        assert!(!default.iter().any(|v| v == "removeViewBox"));
        assert_eq!(catalog["opt_in_status"], "off_by_default");
        assert!(catalog["param_hints"]
            .as_object()
            .unwrap()
            .contains_key("prefixIds"));

        let missing = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/read",
            "params": { "uri": "minsvg://nope" },
        }))
        .unwrap();
        assert_eq!(missing["error"]["code"], -32002);
        assert_eq!(missing["error"]["data"]["uri"], "minsvg://nope");

        let templates = handle_rpc(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/templates/list",
        }))
        .unwrap();
        assert_eq!(
            templates["result"]["resourceTemplates"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn ndjson_roundtrip_lists_plugins_and_optimizes() {
        let input = format!(
            "{}\n{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc":"2.0",
                "id":2,
                "method":"tools/call",
                "params":{"name":"minsvg_list_plugins","arguments":{}}
            }),
        );
        let mut out = Vec::new();
        serve(&mut Cursor::new(input.into_bytes()), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Content-Length:"), "{text}");
        assert!(
            text.contains("minsvg_list_plugins") || text.contains("removeComments"),
            "{text}"
        );
        assert!(text.contains("removeViewBox"), "{text}");
        assert!(text.contains("\"default\""), "{text}");
        assert!(text.contains("\"opt_in\""), "{text}");
    }

    #[test]
    fn optimize_requires_svg_or_path() {
        let resp = call("minsvg_optimize", json!({}));
        assert_eq!(resp["result"]["isError"], true);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("svg") && text.contains("path"), "{text}");
    }

    #[test]
    fn tools_list_exposes_params_config_and_write() {
        let listed = tools_list();
        let tools = listed["tools"].as_array().unwrap();
        let optimize = tools
            .iter()
            .find(|t| t["name"] == "minsvg_optimize")
            .unwrap();
        let batch = tools.iter().find(|t| t["name"] == "minsvg_batch").unwrap();
        assert!(optimize["inputSchema"]["properties"]["params"].is_object());
        assert!(optimize["inputSchema"]["properties"]["config"].is_object());
        assert!(optimize["inputSchema"]["properties"]["param"].is_object());
        assert!(batch["inputSchema"]["properties"]["write"].is_object());
        assert!(batch["inputSchema"]["properties"]["params"].is_object());
        assert_eq!(batch["annotations"]["readOnlyHint"], false);
        assert_eq!(optimize["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn optimize_accepts_plugin_params_object() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"box\" width=\"10\" height=\"10\"/></svg>";
        let resp = call(
            "minsvg_optimize",
            json!({
                "svg": svg,
                "plugin": "prefixIds",
                "skip": "cleanupIds",
                "params": { "prefixIds": { "prefix": "x" } },
            }),
        );
        assert_eq!(resp["error"], Value::Null);
        let out = payload(&resp)["svg"].as_str().unwrap().to_string();
        assert!(
            out.contains("id=\"x__box\"") || out.contains("id='x__box'"),
            "{out}"
        );
    }

    #[test]
    fn optimize_accepts_plugin_inline_json_and_object() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"box\" width=\"10\" height=\"10\"/></svg>";
        let inline = call(
            "minsvg_optimize",
            json!({
                "svg": svg,
                "skip": ["cleanupIds"],
                "plugin": "prefixIds:{\"prefix\":\"y\"}",
            }),
        );
        let inline_svg = payload(&inline)["svg"].as_str().unwrap().to_string();
        assert!(inline_svg.contains("y__box"), "{inline_svg}");

        let object = call(
            "minsvg_optimize",
            json!({
                "svg": svg,
                "skip": "cleanupIds",
                "plugin": { "name": "prefixIds", "params": { "prefix": "z" } },
            }),
        );
        let object_svg = payload(&object)["svg"].as_str().unwrap().to_string();
        assert!(object_svg.contains("z__box"), "{object_svg}");
    }

    #[test]
    fn optimize_accepts_config_path() {
        let root = tempfile::tempdir().unwrap();
        let cfg = root.path().join("minsvg.config.toml");
        fs::write(&cfg, "skip = [\"removeComments\"]\n").unwrap();
        let resp = call(
            "minsvg_optimize",
            json!({
                "svg": TINY,
                "config": cfg.to_string_lossy(),
            }),
        );
        assert_eq!(resp["error"], Value::Null);
        let svg = payload(&resp)["svg"].as_str().unwrap().to_string();
        assert!(svg.contains("note"), "{svg}");
    }

    #[test]
    fn batch_does_not_write_unless_write_true() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.svg");
        fs::write(&a, TINY).unwrap();

        let dry = call(
            "minsvg_batch",
            json!({ "folder": root.path().to_string_lossy() }),
        );
        let dry_body = payload(&dry);
        assert_eq!(dry_body["written"], false);
        assert_eq!(dry_body["written_count"], 0);
        assert_eq!(fs::read_to_string(&a).unwrap(), TINY);

        let with_output = call(
            "minsvg_batch",
            json!({
                "folder": root.path().to_string_lossy(),
                "output": root.path().join("out").to_string_lossy(),
            }),
        );
        assert_eq!(payload(&with_output)["written"], false);
        assert!(!root.path().join("out").join("a.svg").exists());
        assert_eq!(fs::read_to_string(&a).unwrap(), TINY);

        let written = call(
            "minsvg_batch",
            json!({
                "folder": root.path().to_string_lossy(),
                "write": true,
            }),
        );
        let body = payload(&written);
        assert_eq!(body["written"], true);
        assert_eq!(body["written_count"], 1);
        let on_disk = fs::read_to_string(&a).unwrap();
        assert_ne!(on_disk, TINY);
        assert!(!on_disk.contains("<!--"), "{on_disk}");
        assert!(on_disk.contains("<svg"), "{on_disk}");
    }

    #[test]
    fn batch_write_to_output_leaves_sources() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.svg");
        let out = root.path().join("dest");
        fs::write(&a, TINY).unwrap();
        let resp = call(
            "minsvg_batch",
            json!({
                "folder": root.path().to_string_lossy(),
                "write": true,
                "output": out.to_string_lossy(),
            }),
        );
        assert_eq!(payload(&resp)["written"], true);
        assert_eq!(fs::read_to_string(&a).unwrap(), TINY);
        let dest = fs::read_to_string(out.join("a.svg")).unwrap();
        assert!(!dest.contains("<!--"), "{dest}");
    }

    #[test]
    fn initialized_notification_is_silent() {
        assert!(handle_rpc(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .is_none());
    }
}
