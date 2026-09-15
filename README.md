# minSVG

Clean-room Rust SVG optimizer. Animation-aware by default.

**License:** MIT (see [`LICENSE`](LICENSE)).

This is the **installable package** — a library plus the `minsvg` CLI. It is not the YouTube SVGO-vs-Rust bench. The filmed comparison machine lives in [Gromsi/svgo-rust](https://github.com/Gromsi/svgo-rust).

## Honesty

minSVG is **not** the first Rust SVG optimizer. [oxvg](https://github.com/noahbald/oxvg), [svgm](https://github.com/madebyfrmwrk/svgm), [vexy-vsvg](https://crates.io/crates/vexy-vsvg), and [svgcleaner](https://github.com/RazrFalcon/svgcleaner) already exist.

This crate is an original, **clean-room** plugin pipeline inspired by SVGO-shaped passes (parse → named plugins → serialize). We studied public architecture and docs. We do **not** vendor or copy those projects’ source.

v1 is not full SVGO parity. Motion-unsafe passes (`cleanupIds`, `convertPathData`, `mergePaths`, `convertShapeToPath`, `collapseGroups`, `removeHiddenElems`, numeric path rounding) are skipped when SMIL, CSS `@keyframes`, script/events, or extra-source `#id` refs are detected. Embedded-raster recompress still runs on those documents. We do **not** delete SMIL `visibility="hidden"` frames (stock SVGO `removeHiddenElems` can). Already-tight lossy WebP is left as-is.

Not published on crates.io yet.

## Install

Requires a Rust toolchain (**1.83+**; a C compiler for `oxipng` / `libdeflater`).

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
minsvg --help
```

Reinstall after updates:

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked --force
```

From a local clone:

```bash
cargo install --path . --locked --force
```

## Usage

```bash
minsvg in.svg -o out.svg
minsvg in.svg --report -o out.svg
cat in.svg | minsvg --stdin > out.svg
minsvg - -o out.svg < in.svg
```

Useful flags:

| Flag | What it does |
|---|---|
| `-o`, `--output` | Write a file (omit or `-` for stdout) |
| `--stdin` | Read the SVG from stdin |
| `--report` | Print a one-line summary to stderr |
| `--no-animation-aware` | Run every v1 pass even on SMIL/CSS/script docs |
| `--extra App.jsx` | Keep `#id` / `getElementById` refs from sibling JS/TS/CSS |
| `--skip NAME` | Skip a named pass (repeatable) |

`minsvg plugins` lists the wired pass names. Animation-aware is **on** unless you pass `--no-animation-aware`.

## Can you replace SVGO?

**Not as a drop-in npm package.** SVGO is `svgo` on npm: a CLI *and* `import { optimize } from 'svgo'` with a JSON plugin config, plus a Node plugin ecosystem (Vite/webpack loaders). minSVG is a **Rust crate + `minsvg` binary**.

| Surface | minSVG today |
|---|---|
| CLI | Yes: `minsvg in.svg -o out.svg`. stdin/stdout pipes work (`cat in.svg \| minsvg --stdin`, or `minsvg -`). |
| Rust library | Yes: call **`minsvg::optimize`** (bytes) or **`optimize_str`**. `optimize_with` is the same function. |
| npm / wasm / napi | **No.** |
| HTTP / hosted CDN | **No.** The `:8787` race UI is [svgo-rust](https://github.com/Gromsi/svgo-rust), a local bench, not a product API. |
| Plugin config | `--skip NAME` and `--extra file` only. `Config.preset` is unused. Not SVGO’s `plugins: [...]`. |

Embedders should call **`optimize` / `optimize_str`** and read `OptimizeOutput.svg`. The crate also `pub`s AST + named passes for the bench; that surface is awkward — do not depend on it.

This is **not** a pixel-perfect SVGO clone and **not** the first Rust SVG optimizer (see Honesty). It is also **not** an XSS sanitizer: `<script>` and `on*` handlers stay (they trip animation-aware skips). For untrusted uploads, run a real sanitizer *before* or *after* minify.

**Build / Vite / webpack / CI:** yes if you install the binary (`cargo install` or drop `minsvg` on PATH) and add a script. Not a drop-in `svgo` npm package today.

**CDN / SaaS uploads:** run `minsvg` in *your* worker (spawn the CLI or call the Rust lib). We do **not** ship a hosted CDN or a multi-tenant HTTP API. Animation-aware is on by default (`--no-animation-aware` to run every v1 pass). The installable crate does **not** have the bench’s 3% `--no-visual-budget` flag — path minify here is the conservative lossless set.

### CI / npm script

```json
{
  "scripts": {
    "optimize-svg": "find src -name '*.svg' -print0 | xargs -0 -n1 -I{} minsvg {} -o {}"
  }
}
```

```yaml
# GitHub Actions
- uses: dtolnay/rust-toolchain@stable
- run: cargo install --git https://github.com/Gromsi/minSVG --locked
- run: find assets -name '*.svg' -print0 | xargs -0 -n1 -I{} minsvg {} -o {}
```

### CDN worker (sketch)

```bash
# read object bytes → optimize → write. Not a hosted CDN.
minsvg --stdin --report -o /tmp/out.svg < "$OBJECT_BYTES"
# or: cat "$OBJECT_BYTES" | minsvg --stdin > /tmp/out.svg
```

### SaaS upload handler (sketch)

```rust
use minsvg::{optimize, Config};

fn optimize_upload(bytes: &[u8]) -> Result<Vec<u8>, minsvg::OptimizeError> {
    let mut cfg = Config::default(); // animation_aware: true
    // cfg.animation_aware = false; // same as --no-animation-aware
    Ok(optimize(bytes, &cfg)?.svg.into_bytes())
}
```

## Library

```rust
use minsvg::{optimize_str, Config};

let out = optimize_str(include_str!("icon.svg"))?;
println!("{}", out.svg);

let mut cfg = Config::default(); // animation_aware: true
cfg.extra_sources.push(("App.jsx".into(), jsx_source));
let out = minsvg::optimize(svg_bytes, &cfg)?;
```

## Develop

```bash
cargo test
cargo build --release
./target/release/minsvg --help
```

Tests use tiny in-repo fixtures only. They do not download maps or coats of arms.
