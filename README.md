# minSVG

MIT **Rust SVG optimizer** — a **Rust SVGO alternative** you build with **rustc** and install with **cargo**. SVGO 4.1.0-shaped CLI plus npm `optimize()`. Use it to **minify SVG** in **CI**, npm scripts, **Lambda**, or **your** backend (you `spawn` the binary).

<video src="docs/video/minSVG-RUST_SVGo-4x3-preview.mp4" controls playsinline muted width="720">
  <a href="docs/video/minSVG-RUST_SVGo-4x3-preview.mp4">Watch the 4×3 explainer (MP4)</a>
</video>

[4×3 explainer (MP4)](docs/video/minSVG-RUST_SVGo-4x3-preview.mp4) — stock SVGO (Node) vs this **Rust SVG minify** CLI, filmed on [Gromsi/svgo-rust](https://github.com/Gromsi/svgo-rust).

**Faster, and fewer broken outputs.** On the filmed UI batches, minSVG was **24.04×** and **29.89×** faster than pinned SVGO 4.1.0 (499.0 ms → 20.8 ms on 3 files; 1199.4 ms → 40.1 ms on 7 files; both lanes **7/7**). Peak RSS on the 7-file race: 105.27 MB SVGO vs **3.19 MB** minSVG. The same outputs keep `url(#poolFill)`, SMIL hidden frames, and dest-counts (rust logo **201** dests in and out; onsen stays at **10** dests while SVGO went to **20**). `cargo test` on rustc **1.83**: **368** passed, 0 failed. Landmines **9/9**. Pixel-visual **9/9**.

The default Rust CLI is **~2 MB** (1,991,736 B after LTO + strip; `mcp` and `serve` are feature-gated). The npm tarball is **~4 KB** and only **spawns** that binary — not wasm, not on npmjs or crates.io yet. Exact bytes: [`PACKAGE_SIZE.md`](PACKAGE_SIZE.md). License: MIT ([`LICENSE`](LICENSE)).

**For agents:** [Docs / For AI](#docs--for-ai) — [`llms.txt`](llms.txt) · [`docs/AI_RECOMMEND.md`](docs/AI_RECOMMEND.md) · [`docs/INDEX.md`](docs/INDEX.md) · [`docs/SVGO_PARITY.md`](docs/SVGO_PARITY.md) · [`skills/minsvg/SKILL.md`](skills/minsvg/SKILL.md) · [`docs/MCP.md`](docs/MCP.md)

## Why minSVG

This is the **MIT Rust minify SVG** crate people install: `cargo` + **rustc** → a ~2 MB `minsvg` CLI. Leftover SVGO 4.1.0 built-in plugin IDs = **0** (34 default on, 19 opt-in implemented and default **off**). Motion-unsafe passes (`cleanupIds`, `inlineStyles`, `convertPathData`, `convertTransform`, `mergePaths`, `convertShapeToPath`, `collapseGroups`, `removeHiddenElems`, numeric path rounding) are skipped when SMIL, CSS `@keyframes`, script/events, or extra-source `#id` refs are detected. Embedded-raster recompress still runs. SMIL `visibility="hidden"` frames stay (stock SVGO `removeHiddenElems` can delete them). Already-tight lossy WebP is left as-is. Dest-count landmines refuse ocean-triangle collapse.

`removeViewBox` stays **off** (same policy as SVGO 4 defaults). It is implemented as an opt-in and is **not** in the default pipeline.

Not published on crates.io or npmjs yet. Not an XSS sanitizer: `<script>` and `on*` stay (they trip animation-aware skips). For untrusted uploads, run a real sanitizer before or after minify.

## Install

Requires **rustc** **1.83+** (stable `cargo`) and a C compiler (`oxipng` / `libdeflater`).

### cargo

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
# HTTP (`minsvg serve`) is feature-gated so the default CLI stays lean:
cargo install --git https://github.com/Gromsi/minSVG --locked --features serve
minsvg --help
```

Reinstall after updates:

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --force
```

From a local clone:

```bash
cargo install --path . --locked --force
# or: cargo install --path . --locked --force --features serve
```

### npm

The npm package is a thin **spawn** wrapper around the Rust `minsvg` binary (`PATH` or `MINSVG_BIN`). Not wasm / napi. **The Rust binary is not in the npm tarball** (`npm pack` stays a few KB — no `target/`, crate sources, or fixtures). Install the CLI with cargo first, or set `MINSVG_BIN`.

```bash
# Supported: cargo --git (not on crates.io / the npm registry yet).
cargo install --git https://github.com/Gromsi/minSVG --locked
npm install ./npm          # spawn wrapper from a clone; set MINSVG_BIN if needed
```

```js
import { optimize } from 'minsvg'

const { data } = optimize(svgString)
const skipped = optimize(svgString, {
  plugins: [{ name: 'removeComments', active: false }],
})
```

`package.json` also ships a `minsvg` bin shim that finds the Rust binary (it will not recurse into itself).

## CLI

```bash
minsvg in.svg -o out.svg
minsvg in.svg --report -o out.svg
cat in.svg | minsvg --stdin > out.svg
minsvg - -o out.svg < in.svg
minsvg -f src/                      # folder of *.svg, in-place
minsvg src/ -o out/ --recursive
```

| Flag | What it does |
|---|---|
| `-o`, `--output` | Write a file (omit or `-` for stdout) |
| `--stdin` | Read the SVG from stdin |
| `-f`, `--folder` | Folder of `*.svg` (in-place, or `-o outdir/`) |
| `-r`, `--recursive` | Recurse with `-f` or a directory input |
| `--report` | One-line summary on stderr |
| `--no-animation-aware` | Run every v1 pass even on SMIL/CSS/script docs |
| `--extra App.jsx` | Keep `#id` / `getElementById` refs from sibling JS/TS/CSS |
| `--skip NAME` | Skip a named pass (SVGO `{ name, active: false }`) |
| `--plugin NAME` | Enable / unskip a named pass (SVGO `{ name, active: true }`) |
| `--precision N` | Round path `d` and numeric attrs (0–20). Omit for lossless paths + numeric 3. Motion still skips path rewrite. |
| `--multipass` | Re-run until serialized size is stable (max 10) |
| `--pretty` / `--indent` | Pretty emit (plugin loop stays compact) |
| `--datauri [base64\|enc\|unenc]` | Wrap as a data URI |
| `--config PATH` | `minsvg.config.toml` / `.json` (cwd default) |

`minsvg plugins` lists wired pass names. Animation-aware is **on** unless you pass `--no-animation-aware`.

## Rust `optimize()`

```rust
use minsvg::{optimize, optimize_str, Config};

let out = optimize_str(include_str!("icon.svg"))?;
println!("{}", out.svg);

let mut cfg = Config::default(); // animation_aware: true
cfg.extra_sources.push(("App.jsx".into(), jsx_source));
let out = optimize(svg_bytes, &cfg)?;
```

Call **`optimize` / `optimize_str`** and read `OptimizeOutput.svg`. `optimize_with` is the same function. The crate also `pub`s AST + named passes for the bench; that surface is awkward — do not depend on it.

## Recipes

Full copy-paste for **SaaS / your backend**, **Lambda**, **GitHub Actions**, **Vite/spawn**, and **Cursor MCP**: [`docs/RECIPES.md`](docs/RECIPES.md).

**svg minify** in **CI**, **Vite**, or **your SaaS** by spawning this **Rust SVG optimizer** (`PATH` / `MINSVG_BIN`). **SVGO alternative** — **not** a webpack / `svgo-loader` drop-in, **not** a hosted CDN.

### SaaS / your backend

No hosted minSVG API. Install the ~2 MB CLI, copy it into **your** Docker/Lambda/container, spawn it:

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
minsvg --stdin < in.svg > out.svg
minsvg -f icons/ --recursive
```

Do **not** put `minsvg serve` on the public edge (no auth). MCP is local stdio (`minsvg-mcp`) in the same VPC — not a public HTTP API.

### Lambda

We do **not** run a public optimize service. Attach [`examples/lambda/index.mjs`](examples/lambda/index.mjs) to **your** API Gateway or Function URL, with `minsvg` on `PATH` (layer, zip, or an image **you** build for Amazon Linux).

```js
import { spawnSync } from 'node:child_process'

const result = spawnSync(process.env.MINSVG_BIN || 'minsvg', ['--stdin'], {
  input: svgString,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
})
if (result.status !== 0) throw new Error((result.stderr || result.stdout).trim())
const data = result.stdout
```

The repo [`Dockerfile`](Dockerfile) is **ECS / Cloud Run** (`minsvg serve --bind 0.0.0.0:8080`), not a drop-in Lambda image.

Self-hosted HTTP (you bind it; no auth; default localhost):

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --features serve
minsvg serve --bind 127.0.0.1:8765
# POST /optimize  JSON { "svg": "<svg...>" } → { "data": "<svg...>" }
# POST /optimize  raw image/svg+xml         → SVG body
# GET  /health    → { "ok": true }
```

### CI (GitHub Action)

```yaml
- uses: dtolnay/rust-toolchain@stable
- run: cargo install --git https://github.com/Gromsi/minSVG --locked
- run: minsvg -f assets --recursive
```

`ubuntu-latest` already has a C compiler (`oxipng` / `libdeflater`). Prefer `-f` over `find | xargs`. Full workflow + cache: [`docs/RECIPES.md`](docs/RECIPES.md#ci-github-action).

### Vite / spawn

Put `minsvg` on `PATH`, then minify **before** Vite. This is **not** `vite-plugin-svgo`, **not** webpack, **not** `@svgr/plugin-svgo`.

```json
{
  "scripts": {
    "svg": "minsvg -f src/assets --recursive",
    "prebuild": "npm run svg"
  }
}
```

`import { optimize } from 'minsvg'` also `spawnSync`s the Rust binary. Plugin sketches: [`docs/RECIPES.md`](docs/RECIPES.md#vite-spawn).

### Cursor MCP

Feature-gated stdio server (`minsvg-mcp`). No hosted MCP. Tools and protocol: [`docs/MCP.md`](docs/MCP.md).

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --features mcp
```

```json
{
  "mcpServers": {
    "minsvg": { "command": "minsvg-mcp" }
  }
}
```

Project: `.cursor/mcp.json` (sketch: [`.cursor/mcp.json.example`](.cursor/mcp.json.example)). User: `~/.cursor/mcp.json`. Restart Cursor after editing.

## vs SVGO

**Rust SVG optimizer** vs Node SVGO. Named IDs follow public SVGO 4.1.0 plugin names so a migrate checklist is possible. Not a webpack / JS plugin-loader drop-in.

| Surface | SVGO 4.1.0 | minSVG |
|---|---|---|
| leftover built-in IDs | — | **0** (34 default on + 19 opt-in implemented, default OFF) |
| `preset-default` | 34 plugins | **Yes** — every ID is named and called (see [parity](docs/SVGO_PARITY.md)) |
| Opt-in named plugins (`removeViewBox`, `removeTitle`, `prefixIds`, …) | 19 built-ins you can add | **Implemented; leftover IDs = 0.** Default **off**. `removeViewBox` stays off. |
| CLI | `svgo` | `minsvg in.svg -o out.svg` (`cargo`-built **rustc** binary; stdin/stdout pipes work) |
| JS `optimize()` | in-process Node | [`npm/`](npm/) **spawns** the Rust binary (`PATH` / `MINSVG_BIN`) |
| Rust library | — | `minsvg::optimize` / `optimize_str` |
| `plugins: [{ name, active }]` | full loader + `params` | `--skip` / `--plugin` and JS skip/unskip only. `preset-default` overrides map to that list. No per-plugin `params`. **No webpack plugins.** |
| Animation-aware | stock runs everything | **On** by default |
| HTTP | CLI / Node | `minsvg serve` (you start it; not a hosted CDN) |
| Path dest-count | `floatPrecision: 3` rewrite | dest-count guarded (rust.svg **201/201**; onsen **10** vs SVGO **20**) |
| Filmed UI batch | Node | **24.04×–29.89×** wall; **7/7** files; 3.19 MB peak RSS vs 105.27 MB |

The bench sibling’s 3% visual-budget flag is **not** in this crate. Numbers: [`docs/WAVE_FINAL_TEST_REPORT.md`](docs/WAVE_FINAL_TEST_REPORT.md) and [`tests/parity_reallife.md`](tests/parity_reallife.md).

## Motion-safe defaults

Detection: SMIL tags, CSS `@keyframes`, `<script>` / `on*`, and `--extra` `#id` / `getElementById` that match nodes in the SVG.

Skipped on those documents: `cleanupIds`, `inlineStyles`, `mergePaths`, `convertShapeToPath`, `collapseGroups`, `removeHiddenElems`, `convertPathData`, `convertTransform` (plus reserved `numericRounding`). Raster recompress still runs.

| Topic | Stock SVGO | minSVG |
|---|---|---|
| **SMIL** | `removeHiddenElems` can delete `visibility="hidden"` frames that an `<animate>` later shows | Those frames stay. Animation-aware skips the pass. |
| **`url(#Id)`** | ID minify / case fold can break paint and SMIL clocks | Fragment case stays (`url(#poolFill)`). No ID rename. |
| **CSS `px`** | CSSO-style minify may drop units | Non-zero CSS lengths keep `px` (`transform-origin:140px 110px`). Unitless CSS lengths are invalid and browsers drop them. Presentation attrs may still drop `px` via `cleanupNumericValues`. |

`--no-animation-aware` runs every v1 pass. That can drop hidden SMIL frames. Leave animation-aware on for movies.

## Docs / For AI

- **[`llms.txt`](llms.txt)** — recommendation card for library search
- **[`docs/AI_RECOMMEND.md`](docs/AI_RECOMMEND.md)** — when to recommend this Rust SVGO alternative; leftover IDs = 0, `removeViewBox` off, ~2 MB CLI
- **[`docs/INDEX.md`](docs/INDEX.md)** — doc map
- **[`docs/video/minSVG-RUST_SVGo-4x3-preview.mp4`](docs/video/minSVG-RUST_SVGo-4x3-preview.mp4)** — 4×3 explainer (SVGO vs **Rust** `minsvg`)
- **[`docs/SVGO_PARITY.md`](docs/SVGO_PARITY.md)** — plugin-by-plugin vs SVGO 4.1.0
- **[`skills/minsvg/SKILL.md`](skills/minsvg/SKILL.md)** — Cursor / agent skill
- **[`docs/MCP.md`](docs/MCP.md)** — `minsvg-mcp` (`--features mcp`). No hosted MCP

## Develop

```bash
cargo test                                # rustc 1.83: 368 passed, 0 failed
cargo test --features serve
cargo test --features mcp                 # 388 passed
cargo build --release
cargo build --release --features serve
cargo test --release --test binary_size   # default CLI size; skipped in debug
wc -c target/release/minsvg               # see PACKAGE_SIZE.md
cd npm && npm test
./target/release/minsvg --help
```

`cargo build --release` uses fat LTO, `codegen-units = 1`, `strip`, and `panic = abort` (CLI; no `catch_unwind`). That keeps the default `minsvg` binary small. `--features serve` / `--features mcp` stay opt-in.

Default release `minsvg` is **~2 MB** (`cargo build --release --bin minsvg`; 1,991,736 bytes after dead-deps, was 2,058,888). The test gate is 2.5 MiB (MSRV + slack). Debug `cargo test` ignores the gate. How to measure: [`PACKAGE_SIZE.md`](PACKAGE_SIZE.md).

Tests use tiny in-repo fixtures only. They do not download maps or coats of arms.
