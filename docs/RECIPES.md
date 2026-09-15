# minSVG recipes

Copy-paste for **svg minify** with a **Rust SVG optimizer** / **SVGO alternative**: **Lambda** (`spawn`), **CI** (**GitHub Action**), **Vite/spawn**, and **Cursor MCP**.

This is **not** the first Rust SVG optimizer ([oxvg](https://github.com/noahbald/oxvg), [svgm](https://github.com/madebyfrmwrk/svgm), [vexy-vsvg](https://crates.io/crates/vexy-vsvg), [svgcleaner](https://github.com/RazrFalcon/svgcleaner)). It is **not** a webpack / `svgo-loader` / `vite-plugin-svgo` drop-in. There is **no** hosted CDN — you start `minsvg` (or `minsvg-mcp`) yourself.

The npm helper `optimize()` **spawns** the Rust binary (`PATH` or `MINSVG_BIN`). It is not wasm / napi / in-process SVGO.

| Recipe | You run |
|---|---|
| [Lambda](#lambda) | Node handler + `minsvg --stdin` on PATH |
| [CI (GitHub Action)](#ci-github-action) | `cargo install` + `minsvg -f` |
| [Vite spawn](#vite-spawn) | `prebuild` script, or a tiny plugin that calls `optimize()` |
| [Cursor MCP](#cursor-mcp) | `minsvg-mcp` via `--features mcp` — [docs/MCP.md](MCP.md) |
| [Self-hosted HTTP](#self-hosted-http) | `minsvg serve` (`--features serve`) on **your** bind |

CLI, flags, and motion-safe defaults: [README](../README.md). Plugin names: [SVGO_PARITY.md](SVGO_PARITY.md).

---

## Lambda

Attach [`examples/lambda/index.mjs`](../examples/lambda/index.mjs) to **your** API Gateway or Function URL. We do not host this and do not open a public URL.

The handler `spawnSync`s `minsvg --stdin` (override with `MINSVG_BIN`). Node **18+**.

| Request | Response |
|---|---|
| `Content-Type: application/json` body `{ "svg": "<svg…>" }` (or a body that starts with `{`) | `{ "data": "<svg…>" }` |
| Raw `image/svg+xml` (or any non-JSON body) | SVG body, `image/svg+xml` |

Errors: `400` with `{ "error" }` if parse/optimize fails; `500` if the binary is missing.

```js
import { spawnSync } from 'node:child_process'

const result = spawnSync(process.env.MINSVG_BIN || 'minsvg', ['--stdin'], {
  input: svgString,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
})
```

### Binary on PATH

Lambda does not include `minsvg`. Put a **Linux** binary (same arch as the function: `x86_64` or `arm64`) on PATH:

| Pack | Layout |
|---|---|
| Layer | `bin/minsvg` in the zip → `/opt/bin/minsvg` (PATH already includes `/opt/bin`) |
| Zip | Bundle `minsvg` next to the handler and set `MINSVG_BIN` |
| Container | COPY the binary into a **Node Lambda** base image (`public.ecr.aws/lambda/nodejs:20`, …) |

Build on **Amazon Linux 2023** (or a matching container), not on macOS. You need Rust **1.83+** and a C compiler (`oxipng` / `libdeflater`):

```bash
# inside amazonlinux:2023 (or an AL2023 builder)
dnf install -y gcc gcc-c++ git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.83.0
source "$HOME/.cargo/env"
cargo install --git https://github.com/Gromsi/minSVG --locked
# ~/.cargo/bin/minsvg  →  layer/bin/minsvg
```

The repo [`Dockerfile`](../Dockerfile) is **ECS / Cloud Run**: `CMD ["minsvg", "serve", "--bind", "0.0.0.0:8080"]`. It is **not** a drop-in Lambda image (no Lambda RIC / handler). Do not point a Function URL at that Debian `serve` image and expect AWS to invoke it.

Sync payload limits (API Gateway / Lambda, typically 6 MB) are tighter than the handler’s 32 MB `maxBuffer`. For untrusted uploads, run a real XSS sanitizer; minSVG is not one (`<script>` / `on*` stay).

---

## CI (GitHub Action)

MSRV is **1.83+**. `ubuntu-latest` already has a C compiler.

Prefer the folder walk: `minsvg -f assets --recursive` (in-place). No `find | xargs` required.

```yaml
# .github/workflows/svg-minify.yml
name: svg-minify

on:
  pull_request:
  push:
    branches: [main]

jobs:
  minify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.83"

      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/minsvg
          key: minsvg-1.83-${{ runner.os }}-${{ runner.arch }}

      - name: Install minSVG
        run: |
          if ! command -v minsvg >/dev/null 2>&1; then
            cargo install --git https://github.com/Gromsi/minSVG --locked
          fi

      - name: Optimize SVGs
        run: minsvg -f assets --recursive

      - name: Fail if checked-in SVGs are not already minified
        run: git diff --exit-code -- '*.svg'
```

Drop the `git diff` step if this job is allowed to rewrite files (and you commit the result in a later step). Local npm script:

```json
{
  "scripts": {
    "optimize-svg": "minsvg -f src --recursive"
  }
}
```

`npm install minsvg` (or `./npm` from a clone) is only the spawn shim. CI still needs the Rust binary from `cargo install` (or `MINSVG_BIN`).

---

## Vite spawn

minSVG does **not** ship a Vite or webpack plugin. Do not swap this in for `vite-plugin-svgo`, `svgo-loader`, `ImageMinimizerPlugin`, or `@svgr/plugin-svgo`.

Install the Rust binary, then either minify **before** Vite (recommended) or call `optimize()` from a plugin you own. The helper `spawnSync`s `minsvg` at **build** time; the browser never sees the binary.

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
# optional shim: npm install minsvg   # or npm install ./npm from a clone
```

### `prebuild` (recommended)

Leaves Vite’s SVG-as-URL pipeline alone. Works with SVGR: minify files on disk, then let SVGR import them.

```json
{
  "scripts": {
    "svg": "minsvg -f src/assets --recursive",
    "prebuild": "npm run svg",
    "build": "vite build"
  }
}
```

### Emitted assets

`vite build` only (not `vite dev`). Spawns once per emitted `.svg` asset.

```js
// vite.config.js — you own this plugin. Not a published vite-plugin-minsvg.
import { defineConfig } from 'vite'
import { optimize } from 'minsvg'

function minsvgAssets(options = {}) {
  return {
    name: 'minsvg-assets',
    generateBundle(_opts, bundle) {
      for (const item of Object.values(bundle)) {
        if (item.type !== 'asset') continue
        if (!String(item.fileName).endsWith('.svg')) continue
        const source =
          typeof item.source === 'string'
            ? item.source
            : Buffer.from(item.source).toString('utf8')
        item.source = optimize(source, options).data
      }
    },
  }
}

export default defineConfig({
  plugins: [minsvgAssets()],
})
```

### `?raw` imports

Spawns on each transform (including dev). Prefer `prebuild` for large icon sets.

```js
function minsvgRaw(options = {}) {
  return {
    name: 'minsvg-raw',
    enforce: 'pre',
    transform(code, id) {
      const [file, query = ''] = id.split('?')
      if (!file.endsWith('.svg')) return null
      if (!/(?:^|&)raw(?:&|$)/.test(query)) return null
      const { data } = optimize(code, options)
      return { code: `export default ${JSON.stringify(data)}`, map: null }
    },
  }
}
```

`optimize(svg, { plugins: [{ name: 'removeComments', active: false }] })` maps onto `--skip` / `--plugin` only. Missing binary → `ENOENT` and a message to `cargo install` or set `MINSVG_BIN`.

---

## Cursor MCP

Cursor-compatible **stdio** JSON-RPC. Feature-gated so the default `minsvg` CLI stays small. The binary is **`minsvg-mcp`** — it is not linked into `minsvg`. There is **no** hosted MCP endpoint.

Protocol, framing, and tool schemas: [MCP.md](MCP.md). This page is only the install / `mcp.json` recipe.

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --features mcp
# binary: minsvg-mcp
```

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

From a clone without installing (Rust **1.83+**):

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

Restart Cursor (or reload MCP) after editing.

| Tool | What it does |
|---|---|
| `minsvg_optimize` | `svg` **or** `path` → optimized string + byte counts |
| `minsvg_list_plugins` | Wired default names vs SVGO opt-in names (**off**) |
| `minsvg_batch` | Folder of `*.svg` (optional `recursive`). **Does not write files.** |

`skip` / `plugin` match the CLI flags. Animation-aware stays on. Opt-in SVGO names (`removeViewBox`, `removeTitle`, …) stay **off** unless you pass `plugin`. Leftover built-in IDs = 0; `--plugin` does not invent a missing pass.

---

## Self-hosted HTTP

Optional. Same honesty as Lambda: **you** bind it. No auth. Default is localhost.

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --features serve
minsvg serve --bind 127.0.0.1:8765
```

| Method | Path | Body | Reply |
|---|---|---|---|
| `GET` | `/health` | — | `{ "ok": true }` |
| `POST` | `/optimize` | JSON `{ "svg" }` | `{ "data" }` |
| `POST` | `/optimize` | raw `image/svg+xml` | SVG body |

| Where | Command |
|---|---|
| Laptop | `minsvg serve --bind 127.0.0.1:8765` |
| ECS / Cloud Run | [`Dockerfile`](../Dockerfile): `CMD ["minsvg", "serve", "--bind", "0.0.0.0:8080"]` — **you** deploy it |
| Lambda | Spawn handler above — not this `serve` image |

Use `0.0.0.0:8080` only when you opt into LAN / a container. Put **your** nginx or API Gateway in front.
