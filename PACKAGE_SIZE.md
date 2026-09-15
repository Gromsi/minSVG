# minSVG package size

Measured on 2026-09-15, `rustc 1.98.1`, Mach-O arm64. Tests: **97 passed** (`cargo test`). `minsvg --help` and a one-line SVG optimize both work.

## Binary

| | bytes | on disk |
|---|---:|---|
| **Before** (HEAD crate, 21:21 build: clap default features, no custom release profile) | 2,415,144 | 2.3M |
| **After** (`[profile.release]`: `lto=true`, `codegen-units=1`, `strip=true`, `panic="abort"`; clap `default-features=false`) | 1,446,432 | 1.4M |
| **Delta** | −968,712 | **−40%** |

```
wc -c target/release/minsvg
# 1446432 target/release/minsvg
```

The before number is the existing `target/release/minsvg` from the shipped MIT crate (`1558f8c`), taken before sibling slimming. Same host, same rustc.

## Tree (excluding `target/`)

```
du -sh -I target .
# 632K	.
```

| path | size |
|---|---|
| `.git` | 296K |
| `src/` | 280K (266,451 bytes of `.rs`) |
| `tests/` | 24K |
| `Cargo.lock` | 16K (517 lines; −91 vs HEAD) |
| rest (`Cargo.toml`, README, LICENSE, gitignore) | ~16K |
| **source+docs, no `.git`** | **~336K** |

`du -sh .` including `target/` is **1.6G** (debug + release artifacts). That is not the publishable package.

`[package] include` is already narrowed to `src/*.rs`, `tests/*.rs`, `tests/fixtures/*.svg`, lockfile, LICENSE, README.

## What got smaller

- **Release profile**: fat LTO + one CGU + strip + abort. Most of the 968 KB drop is here; the 21:21 binary was a default `cargo build --release`.
- **clap**: `default-features = false` plus `std, derive, help, usage, error-context` only (no color / suggestions / wrap).
- **Source**: sibling passes deleted ~38 KB of Rust (unused exports, duplicated helpers). No merge compile errors.
- **Lockfile**: 91 lines gone with unused clap feature crates.

## Remaining fat

Almost all of the 1.4M binary is **embedded-raster recompress**, not the SVG pipeline.

1. **`oxipng` 9.1.5** (`features = ["parallel"]`) — `libdeflater` / `libdeflate-sys` (C), `rayon`, `bitvec`, `indexmap`. This is the largest remaining crate graph.
2. **`image` 0.25.5** — `png` + `jpeg` + `webp` (`zune-jpeg`, `image-webp`, `png` + **two** `miniz_oxide` versions: 0.8.9 and 0.9.1).
3. **`indexmap` 2.11.4** — direct dep is unused in `minsvg` source (1.83 pin so the index does not pull edition-2024 `indexmap` 2.12+). Same crate is also required by `oxipng`; the pin itself adds lock/manifest weight, not a second copy.
4. **`clap` derive** — still in the binary for `--help` / flags. Small next to raster.
5. **Rust std + libSystem** — only system dylib is `/usr/lib/libSystem.B.dylib`. `__TEXT` ≈ 1.35M of the 1.45M file.

`quick-xml`, `base64`, and `thiserror` are negligible.

## Next cuts (if we need another 30–50%)

- Feature-gate `image` + `oxipng` (`--features raster` / default-on) so `cargo install --no-default-features` is a skinny path-only CLI.
- Drop `oxipng` `parallel` (drop `rayon`) if single-thread PNG is acceptable.
- Drop the unused direct `indexmap` pin once MSRV is 1.85+ (keep oxipng’s transitive copy).
- Unify `miniz_oxide` 0.8 / 0.9 if the `png`/`flate2` stack allows it.

Functionality held: comment + default `#000000` fill stripped from a 10×10 circle; SMIL / path-fidelity tests still green.
