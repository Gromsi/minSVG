# minSVG

Clean-room Rust SVG optimizer. The filmed product and source of truth live in **[Gromsi/svgo-rust](https://github.com/Gromsi/svgo-rust)** (local SVGO vs Rust bench). This repo is the product name, not a second codebase.

We did not invent Rust SVG optimization. oxvg, svgm, vexy-vsvg, and svgcleaner already exist.

## Install

```bash
cargo install --git https://github.com/Gromsi/svgo-rust --locked --bin minsvg
minsvg input.svg -o out.svg
```

Animation-aware is on by default. Same binary as `svg-race` in the bench.

```bash
# npm
find src -name '*.svg' -print0 | xargs -0 -n1 -I{} minsvg {} -o {}

# GitHub Action
# cargo install --git https://github.com/Gromsi/svgo-rust --locked --bin minsvg
```

Not published on crates.io yet. Run the YouTube bench with `./scripts/run_local.sh` in svgo-rust → http://127.0.0.1:8787.