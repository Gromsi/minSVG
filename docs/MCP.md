# minSVG MCP

Cursor-compatible stdio JSON-RPC server. **Feature-gated** (`--features mcp`) so the default `minsvg` CLI stays small. This is a separate `minsvg-mcp` binary — it is not linked into `minsvg`.

## Local only

`minsvg-mcp` is a **local stdio** process. Cursor (or another MCP host) spawns it and talks over stdin/stdout. It does **not** open a TCP port, does not bind `0.0.0.0`, and is not a hosted endpoint.

`minsvg serve` (`--features serve`) is a separate local HTTP helper. It defaults to `127.0.0.1:8765` and has **no auth**. Port-only or wildcard hosts (`:8080`, `*`) are rewritten to loopback so a typo cannot listen on every interface. Use `--bind 0.0.0.0:8080` only when you opt into a container you deploy.

## Install

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --features mcp
# binary: minsvg-mcp
```

From a clone (Rust **1.83+**):

```bash
cargo build --release --features mcp --bin minsvg-mcp
cargo test --features mcp
```

## Cursor `mcp.json`

Project: `.cursor/mcp.json`. User: `~/.cursor/mcp.json`.

```json
{
  "mcpServers": {
    "minsvg": {
      "command": "minsvg-mcp"
    }
  }
}
```

From a local clone without installing:

```json
{
  "mcpServers": {
    "minsvg": {
      "command": "cargo",
      "args": [
        "run",
        "--quiet",
        "--release",
        "--features",
        "mcp",
        "--bin",
        "minsvg-mcp"
      ],
      "cwd": "/absolute/path/to/minSVG"
    }
  }
}
```

Restart Cursor (or reload MCP) after editing `mcp.json`.

## Tools

| Tool | Arguments | Result |
|---|---|---|
| `minsvg_optimize` | `svg` **or** `path`; optional `skip` / `plugin` / `params` / `param` / `config` | Optimized SVG **string** + **bytes** (`bytes`, `input_bytes`) |
| `minsvg_list_plugins` | _(none)_ | `default` (wired) vs `opt_in` (SVGO names; **off** unless `plugin`) plus `param_hints`. Same catalog: `minsvg://plugins` |
| `minsvg_batch` | `folder`; optional `skip` / `plugin` / `params` / `param` / `config` / `recursive` / `write` / `output` | Each `*.svg` optimized (string + bytes). **Does not write** unless `write` is `true`. |

`skip` / `plugin` match the CLI `--skip` / `--plugin` flags. `plugin` also accepts `name:{"foo":1}` or `{ "name", "params" }`.

Per-plugin JSON (same as CLI `--param`; does **not** enable a pass — pair with `plugin`):

```json
{
  "svg": "<svg …>",
  "plugin": "prefixIds",
  "params": { "prefixIds": { "prefix": "icon" } }
}
```

`param` is the CLI string form (`prefixIds={"prefix":"icon"}`) or the same object shape as `params`. `config` is a path to `minsvg.config.toml` / `.json`. Call `minsvg_list_plugins` for `param_hints`.

`minsvg_batch` never overwrites files unless `write` is the boolean `true`. With `write: true` and no `output`, it writes in place. With `write: true` and `output`, it writes into that directory and leaves the sources alone. `output` without `write: true` is ignored.

Animation-aware stays on. Opt-in SVGO plugins (`removeViewBox`, `removeTitle`, …) stay **off** unless you pass them as `plugin`.

## Prompts

User-selected templates (`prompts/list`, `prompts/get`). Advertised in `initialize.capabilities.prompts`.

| Prompt | Arguments | Result |
|---|---|---|
| `minsvg_recommend` | optional `use_case` (icon, SMIL, Lambda, Vite/webpack plugin, custom JS plugins, sanitizer, …) | Messages: when to recommend **minSVG vs SVGO** |

`minsvg_recommend` is the honesty gate: minSVG for Rust / motion-safe defaults / CI / Lambda / MCP; SVGO for a JS plugin loader, official bundler plugins, custom JS plugins, or byte-identical 4.1.0 `floatPrecision: 3`. Neither is a sanitizer. Opt-in names stay off unless the user asks.

## Resources

Application-driven context (`resources/list`, `resources/read`). Advertised in `initialize.capabilities.resources`. `resources/templates/list` is empty (no URI templates).

| URI | MIME | Contents |
|---|---|---|
| `minsvg://plugins` | `application/json` | Same catalog as `minsvg_list_plugins`: `default` (wired) vs `opt_in` (off unless `plugin`), plus `motion_skip` and `param_hints` |

Unknown URIs return JSON-RPC `-32002`. Trailing slash on `minsvg://plugins/` is accepted.

## Protocol

stdio JSON-RPC 2.0 with LSP-style `Content-Length` framing (single-line NDJSON is also accepted). Methods: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`, `prompts/list`, `prompts/get`, `resources/list`, `resources/read`, `resources/templates/list`, `ping`, `shutdown`.
