# minSVG package size

Task 36 (`[profile.release]` only): measured 2026-09-16 with `cargo +1.83.0 build --release --bin minsvg` (default features; MCP/serve off), Mach-O arm64.

| | bytes | on disk |
|---|---:|---|
| **Before** (stock release: no LTO, 16 CGUs, symbols kept, `panic=unwind`) | 3,460,616 | 3.3M |
| **After** (`lto=true`, `codegen-units=1`, `strip=true`, `panic="abort"`) | 2,058,888 | 2.0M |
| **Delta** | −1,401,728 | **−40%** |

```
wc -c target/size-after/release/minsvg
# 2058888 target/size-after/release/minsvg
```

`panic=abort` is kept: this crate is a CLI and does not `catch_unwind`. Library dependents still use *their* profile.

Earlier 2026-09-15 note (`rustc 1.98.1`, slimmer tree): 2,415,144 → 1,446,432 (−40%). The 1.83.0 pair above is the current default `minsvg`.

## Release binary budget

The default `minsvg` CLI (no `--features serve` / `mcp`) must stay at or under **2.5 MiB** (`2_621_440` bytes) after `cargo build --release --bin minsvg`.

That is the current measured size (2,058,888 on rustc 1.83 **and** 1.98.1, Mach-O arm64) plus slack for Linux / rustc variance. The 1.5–2.0 MB band is the product target (an older slimmer tree hit 1.45 MB). The gate is **measured size + slack** so a 1.83 Linux binary does not flake, while a `[profile.release]` revert (3.3M+ stock 1.83) still fails.

**Debug `cargo test` does not enforce this** (unstripped debug `minsvg` is ~14 MB). CI is debug-only and must stay green.

### Measure

```bash
# Host rustc (whatever `rustc --version` is)
cargo build --release --bin minsvg
wc -c target/release/minsvg

# MSRV (same rustc as CI). Needs rustup toolchain 1.83.0.
cargo +1.83.0 build --release --bin minsvg
wc -c target/release/minsvg
```

`wc -c` is the byte count the budget uses. `du -h` is the on-disk block size — do not compare that to the ceiling.

### Enforce

```bash
cargo test --release --test binary_size
```

`tests/binary_size.rs` stats `CARGO_BIN_EXE_minsvg` (same profile as the test). `--features serve` is skipped — that binary is not the default CLI. `minsvg-mcp` is a separate bin and is not budgeted here.

## Binary (2026-09-15 archive)

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

## npm tarball (`npm/`)

The published JS package is a spawn wrapper only. **The Rust `minsvg` binary is optional and is not packed.** Measured 2026-09-16 with `npm pack --dry-run` from `npm/`. `npm run pack:check` enforces 256 KB unpacked / 128 KB packed.

| | bytes |
|---|---:|
| packed (`minsvg-0.1.0.tgz`) | 3,959 |
| unpacked | 11,123 |
| entry count | 5 |

Files: `package.json`, `index.js`, `index.d.ts`, `bin/minsvg.js`, `README.md`. Deny-list (`.npmignore`): `target/`, `*.rs`, `Cargo.*`, fixtures, native `minsvg` bins.

## What got smaller

- **Release profile**: fat LTO + one CGU + strip + abort. On rustc 1.83.0 that is a 1.4 MB drop vs stock release; the 2026-09-15 968 KB drop was the same knobs on a slimmer tree.
- **clap**: `default-features = false` plus `std, derive, help, usage, error-context` only (no color / suggestions / wrap).
- **Source**: sibling passes deleted ~38 KB of Rust (unused exports, duplicated helpers). No merge compile errors.
- **Lockfile**: 91 lines gone with unused clap feature crates.

## Remaining fat

Almost all of the current 2.0M binary is **embedded-raster recompress**, not the SVG pipeline.

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

## Snapshot (2026-09-16)

Re-checked on this host (`rustc 1.98.1`, existing `target/release/minsvg`; `npm run pack:check` from `npm/`).

| artifact | size | notes |
|---|---|---|
| default release CLI (`minsvg`, no extra features) | **~2.0 MB** (2,058,888 bytes) | `cargo build --release --bin minsvg` |
| npm tarball packed | **~4 KB** (3,959) | `minsvg-0.1.0.tgz` |
| npm tarball unpacked | **~11 KB** (11,123) | 5 files: `package.json`, `index.js`, `index.d.ts`, `bin/minsvg.js`, `README.md` |

- **Rust `minsvg` binary is not in the npm tarball.** The JS package is a spawn wrapper; install the CLI separately (`cargo install`). `npm/.npmignore` and `pack-check.mjs` deny `target/`, `*.rs`, `Cargo.*`, and native `minsvg` bins.
- **`mcp` and `serve` are feature-gated.** See [Cargo features (T38)](#cargo-features-t38). Neither is the default CLI budget.

## Cargo features (T38)

`default = []`. Confirmed on **rustc 1.83.0** (2026-09-16, Mach-O arm64):

```bash
cargo +1.83.0 build --release --bin minsvg
```

| build | artifact | bytes |
|---|---|---:|
| default (no features) | `minsvg` | 2,058,888 |
| `--features serve` | `minsvg` | 2,242,040 (+183,152) |
| `--features mcp` | `minsvg-mcp` (separate bin) | 1,875,432 |

Default compile graph has **no `axum`, `tiny_http`, `tokio`, or `mcp_stdio`**. `minsvg-mcp` is not produced. `minsvg serve` on the default bin exits 1 (`without HTTP serve` + `--features serve` hint).

`--features serve` compiles `tiny_http` 0.12.0 (plus `ascii` / `httpdate` / `chunked_transfer`), **not axum**. `--features mcp` adds **no crates** to the `minsvg` CLI graph; it only unlocks the extra stdio bin.

**Check npm pack (do not ship the Rust bin):**

```bash
npm run pack:check
# → ok pack minsvg-0.1.0.tgz packed=3959 unpacked=11123 files=5
```

`pack:check` is `node pack-check.mjs` (256 KB unpacked / 128 KB packed ceiling). Also runs as part of `npm test`.
