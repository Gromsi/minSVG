# minSVG Wave final test report

Final integration after tasks **1–40**. Ran on this machine against rustc **1.83.0** (`90b35a623 2024-11-26`). No plugins were added. `removeViewBox` was **not** enabled by default. Nothing was pushed.

`src/lib.rs` mods, `src/plugins.rs`, and `Cargo.toml` features compiled as siblings left them. The only overlapping-edit fix: `tests/optimize_smoke.rs` assumed `CARGO_BIN_EXE_minsvg_mcp` is injected into every integration crate when `--features mcp` is on. Cargo 1.83 sets that env on some crates (`tests/mcp_stdio.rs`) but not `optimize_smoke`. The mcp-on branch now accepts the sibling `minsvg-mcp` next to `CARGO_BIN_EXE_minsvg`. Default (feature off) still requires the env to be unset.

## Confirm

| Check | Result |
|---|---|
| **34 default SVGO IDs on** | **yes** — all 34 `preset-default` names are in `DEFAULT_PLUGIN_NAMES` and run on `Config::default()` (motion may skip a subset). |
| Default list length | **36** = those 34 + added `collapseWhitespace` + `recompressEmbeddedRasters` |
| **19 opt-in off** | **yes** — `OPTIN_PLUGIN_NAMES.len() == 19`, none in the default set, `Config::default().enable_plugins` empty |
| `removeViewBox` | in the 19; **not** in `DEFAULT_PLUGIN_NAMES`; default `optimize()` keeps `viewBox` |
| Leftover SVGO 4.1.0 built-in IDs | **0** (`tests/optin_catalog.rs`) |
| **MCP feature-gated** | **yes** — `mcp = []`; `[[bin]] minsvg-mcp` `required-features = ["mcp"]`; default `cargo test` does not build `minsvg-mcp` |
| **Default CLI small** | **yes** — `default = []` (no `serve` / `mcp`); rustc 1.83 release `minsvg` **2,058,888** bytes (2.5 MiB budget); `serve` is a fail-closed stub without `--features serve` |

`minsvg plugins` prints the 36 wired names, then the 19 opt-ins (default **OFF**), then `MOTION_SKIP_PLUGINS`.

## Commands (actual)

| Command | Result |
|---|---|
| `cargo +1.83.0 test` | **ok** — **368** passed, **0** failed, **1** ignored |
| `cargo +1.83.0 test --features mcp` | **ok** — **388** passed, **0** failed, **1** ignored |
| `npm test` (in `npm/`) | **ok** — `ok 71` then `ok pack minsvg-0.1.0.tgz packed=3959 unpacked=11123 files=5` |
| `npm pack --dry-run` (in `npm/`) | **ok** — package size **4.0 kB**, unpacked **11.1 kB**, **5** files (`README.md`, `bin/minsvg.js`, `index.d.ts`, `index.js`, `package.json`). No Rust binary / `target/` / fixtures. |
| `cargo +1.83.0 test --test landmines --test e2e_all_flags --test optin_parity --test pixel_visual --test speed_size_gates --test parity_reallife` | **ok** — **43** passed (9+10+8+9+3+4) |

The ignored test is `binary_size::default_release_minsvg_stays_under_budget` (debug bins are unstripped; enforce with `cargo test --release --test binary_size`). Measured default release `minsvg` on this host: **2,058,888** bytes.

Not requested, also green: `cargo +1.83.0 test --features serve --test serve_http` — **3** passed (loopback `/health` + `/optimize`; port-only bind stays on `127.0.0.1`).

## `cargo +1.83.0 test` breakdown

| Target | Passed | Ignored |
|---|---:|---:|
| lib `src/lib.rs` | 259 | 0 |
| bin `src/main.rs` | 26 | 0 |
| `binary_size` | 0 | 1 |
| `e2e_all_flags` | 10 | 0 |
| `landmines` | 9 | 0 |
| `onsen_id_case` | 1 | 0 |
| `optimize_smoke` | 11 | 0 |
| `optin_catalog` | 5 | 0 |
| `optin_parity` | 8 | 0 |
| `parity_bench` | 3 | 0 |
| `parity_reallife` | 4 | 0 |
| `path_fidelity` | 5 | 0 |
| `pixel_visual` | 9 | 0 |
| `speed_size_gates` | 3 | 0 |
| `wave2_parity` | 15 | 0 |
| doc-tests | 0 | 0 |
| **total** | **368** | **1** |

`serve_http` and `mcp_stdio` stay behind `--features serve` / `--features mcp`.

## `cargo +1.83.0 test --features mcp` extras

| Target | Passed |
|---|---:|
| default suite (same as above, plus the extra bins) | 368 (+1 ignored) |
| bin `minsvg-mcp` (`src/mcp_stdio.rs`) | 17 |
| `tests/mcp_stdio.rs` | 3 |
| **total** | **388** (+1 ignored) |

`optimize_smoke::default_cargo_test_does_not_build_minsvg_mcp` passes in both configurations: env unset without `mcp`; `minsvg-mcp` file present with `mcp`.

## Named integration binaries

| `--test` | Passed |
|---|---:|
| `landmines` | 9 |
| `e2e_all_flags` | 10 |
| `optin_parity` | 8 |
| `pixel_visual` | 9 |
| `speed_size_gates` | 3 |
| `parity_reallife` | 4 |
| **sum** | **43** |

## npm

`npm test` runs `node test.mjs && node pack-check.mjs`.

- spawn `optimize()`: **`ok 71`**
- pack gate: **packed=3959** / **unpacked=11123** / **files=5** (budgets 128 KB packed / 256 KB unpacked)

`npm pack --dry-run` filename `minsvg-0.1.0.tgz`. Rust `minsvg` is **not** in the tarball.

## Catalog (wired names)

`DEFAULT_PLUGIN_NAMES` (36, default **on**):

```
removeDoctype, removeXMLProcInst, removeComments, removeMetadata,
removeEditorsNSData, cleanupAttrs, removeEmptyAttrs, removeDeprecatedAttrs,
mergeStyles, inlineStyles, minifyStyles, convertColors, cleanupNumericValues,
removeUnknownsAndDefaults, cleanupIds, removeUselessDefs,
removeNonInheritableGroupAttrs, cleanupEnableBackground,
removeUselessStrokeAndFill, removeHiddenElems, removeEmptyText, removeDesc,
convertEllipseToCircle, convertShapeToPath, moveElemsAttrsToGroup,
moveGroupAttrsToElems, collapseGroups, removeEmptyContainers, removeUnusedNS,
collapseWhitespace, recompressEmbeddedRasters, convertPathData,
convertTransform, mergePaths, sortAttrs, sortDefsChildren
```

`OPTIN_PLUGIN_NAMES` (19, default **off**):

```
removeViewBox, removeDimensions, removeXMLNS, removeXlink, removeTitle,
prefixIds, removeRasterImages, removeOffCanvasPaths, removeScripts,
removeStyleElement, reusePaths, convertOneStopGradients,
addAttributesToSVGElement, addClassesToSVGElement, removeAttrs,
removeAttributesBySelector, removeElementsByAttr, cleanupListOfValues,
convertStyleToAttrs
```

## Tasks 1–40

See [`ROADMAP_40.md`](ROADMAP_40.md). Wave 3 numbers in [`WAVE3_TEST_REPORT.md`](WAVE3_TEST_REPORT.md) were 340 / 360 before Wave 4 binaries (`e2e_all_flags`, `landmines`, `optin_catalog`, `onsen_id_case`, extra smoke, `binary_size` ignore). Playwright bench notes: `svgo-rust/output/scenarios/parity-wave3.md` (3/3 both lanes; onsen `url(#poolFill)` kept; batch wall 499.0 ms SVGO / 20.8 ms minSVG).
