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

## Build-script example (npm + cargo)

Install the binary once in CI or on the machine, then fold it into an npm script:

```json
{
  "scripts": {
    "optimize-svg": "find src -name '*.svg' -print0 | xargs -0 -n1 -I{} minsvg {} -o {}"
  }
}
```

GitHub Actions:

```yaml
- uses: dtolnay/rust-toolchain@stable
- run: cargo install --git https://github.com/Gromsi/minSVG --locked
- run: find assets -name '*.svg' -print0 | xargs -0 -n1 -I{} minsvg {} -o {}
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
