# minSVG Wave 3 test report

Task 30. Ran on this machine against rustc **1.83.0** (`90b35a623 2024-11-26`). Tasks **31–40** were not implemented here. No plugins were added. Nothing was pushed.

Opt-in SVGO names stay **off** on `Config::default()` (`enable_plugins` empty). `Cargo.toml` `default = []`; `mcp` is still a feature. `removeViewBox` is in `OPTIN_PLUGIN_NAMES` (19 names) and is **not** in `DEFAULT_PLUGIN_NAMES` (36 names).

`src/lib.rs` mods and `src/plugins.rs` compiled as siblings left them. No conflict edit was required.

## Commands (actual)

| Command | Result |
|---|---|
| `cargo +1.83.0 test` | **ok** — **340** passed, 0 failed, 0 ignored |
| `cargo +1.83.0 test --features mcp` | **ok** — **360** passed, 0 failed, 0 ignored |
| `npm test` (in `npm/`) | **ok** — `ok 71` (spawn `optimize()` output length) |

Wave 2 was 328 default tests. Wave 3 adds `pixel_visual` (9) + `speed_size_gates` (3) = **+12**. MCP adds 17 `minsvg-mcp` unit tests + 3 `tests/mcp_stdio.rs` (Wave 2 was 15 + 2).

## `cargo +1.83.0 test` breakdown

| Target | Passed |
|---|---:|
| lib `src/lib.rs` | 259 |
| bin `src/main.rs` | 26 |
| `optimize_smoke` | 8 |
| `optin_parity` | 8 |
| `parity_bench` | 3 |
| `parity_reallife` | 4 |
| `path_fidelity` | 5 |
| `pixel_visual` | 9 |
| `speed_size_gates` | 3 |
| `wave2_parity` | 15 |
| doc-tests | 0 |
| **total** | **340** |

`serve_http` stays behind `--features serve` (not in this run).

## `cargo +1.83.0 test --features mcp` extras

| Target | Passed |
|---|---:|
| default suite (same as above) | 340 |
| bin `minsvg-mcp` (`src/mcp_stdio.rs`) | 17 |
| `tests/mcp_stdio.rs` | 3 |
| **total** | **360** |

## Wave 3 (21–30)

| # | Task | Evidence |
|---|---|---|
| 21 | sync bench core | `svgo-rust/crates/svg_race_core` default IDs match minSVG. Opt-in / MCP / config stay in minSVG only (delta listed in that crate’s `plugins.rs`). `MOTION_SKIP`, no SMIL frame wipe, `url(#Id)` case kept. No 25 MB raven copy. |
| 22 | pixel/visual tests | `tests/pixel_visual.rs` + `tests/fixtures/url-id.svg`. dest-count / `url(#Id)` / SMIL frames vs original; 8×8 occupancy (no resvg). **9/9**. |
| 23 | speed/size gates | `tests/speed_size_gates.rs`. In-process `optimize()` only (no SVGO Node). 50 ms icon budget; dest-count/size must not explode. **3/3**. |
| 24 | real-life corpus | `tests/parity_reallife.rs` + `.md`: original three plus `circle_icon` / `heart` / `star` / `check`. POV + raven excluded. **4/4**. |
| 25 | playwright bench | `svgo-rust/output/scenarios/parity-wave3.md`. arrow + plus + onsen (no raven). Both lanes 3/3; `url(#poolFill)` on both. Wall 499.0 ms SVGO / 20.8 ms minSVG. |
| 26 | MCP prompts | `minsvg_recommend` + `minsvg://plugins`. `--features mcp` only. Default CLI unchanged. |
| 27 | skill install path | `skills/minsvg/SKILL.md` is source of truth. `.cursor/skills/minsvg/SKILL.md` is a symlink. |
| 28 | Lambda/CI/Vite | `examples/lambda/index.mjs` (`minsvg --stdin`). `examples/vite/vite.config.js` (`optimize()`). Recipes stay in `docs/RECIPES.md`. |
| 29 | CI 1.83+npm+mcp | `.github/workflows/ci.yml`: rustc 1.83, `cargo test --locked`, `cargo test --locked --features mcp`, `npm test` in `npm/`. No secrets / no huge artifacts. |
| 30 | this report | this file + `docs/ROADMAP_40.md` Wave 3 row. |

## Opt-in still default off

Confirmed by `optin_parity` + `wave2_parity` in the runs above: default `optimize()` keeps `viewBox` / `<title>` / `role="img"`; `removeViewBox` runs only with `--plugin` / a config `plugins` entry. `--param` alone does not enable a pass.
