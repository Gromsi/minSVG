# minSVG 40-task roadmap

Status: tasks **1–40 done**. rustc **1.83.0**. No new plugins. `removeViewBox` stays **off**. Nothing pushed.

Tasks **36–40** are code-size optimizations (binary / npm tarball). They are not feature work.

Exact test counts: [`docs/WAVE_FINAL_TEST_REPORT.md`](WAVE_FINAL_TEST_REPORT.md).

## Wave 1 — opt-in plugins, MCP, skill, wiring

| # | Task | Status |
|---|---|---|
| 1 | meta removers | done |
| 2 | prefixIds | done |
| 3 | raster/scripts/style/offcanvas | done |
| 4 | reusePaths+one-stop | done |
| 5 | add/remove attr plugins | done |
| 6 | lists+params | done |
| 7 | MCP v1 | done |
| 8 | agent skill | done |
| 9 | AI README+llms.txt | done |
| 10 | this wiring+tests | done |

## Wave 2 — params, JS/YAML, CLI, npm, deepen

| # | Task | Status |
|---|---|---|
| 11 | deeper plugin params | done |
| 12 | JS optimize SVGO types | done |
| 13 | YAML/JS config | done |
| 14 | remaining CLI flags | done |
| 15 | npm TS exports | done |
| 16 | MCP batch/params | done |
| 17 | skill deepen | done |
| 18 | README recipes | done |
| 19 | registry keywords | done |
| 20 | tests JS/YAML/MCP | done |

## Wave 3 — bench, visual, CI, recipes

| # | Task | Status |
|---|---|---|
| 21 | sync bench core | done |
| 22 | pixel/visual tests | done |
| 23 | speed/size gates | done |
| 24 | real-life corpus | done |
| 25 | playwright bench | done |
| 26 | MCP prompts | done |
| 27 | skill install path | done |
| 28 | Lambda/CI/Vite recipes | done |
| 29 | CI 1.83+npm+mcp | done |
| 30 | test report | done |

## Wave 4 — catalog, localhost, goldens, leftover

| # | Task | Status |
|---|---|---|
| 31 | 1:1 catalog+honesty | done |
| 32 | serve/MCP localhost | done |
| 33 | goldens+landmines | done |
| 34 | all tests pass | done |
| 35 | leftover=0 opt-ins exist | done |

## Wave 5 — code-size optimizations

| # | Task | Status |
|---|---|---|
| 36 | LTO/strip/panic | done |
| 37 | dead deps | done |
| 38 | feature-flag MCP/serve | done |
| 39 | binary size budget | done |
| 40 | npm tarball budget | done |

## Wave 4–5 evidence (final)

| # | Evidence |
|---|---|
| 14 | `src/main.rs` SVGO-shaped flags + `tests/e2e_all_flags.rs` (10). |
| 17 | `skills/minsvg/SKILL.md` (`.cursor/skills/minsvg/SKILL.md` symlink). |
| 18 | `docs/RECIPES.md` + README recipes. |
| 31 | `docs/SVGO_PARITY.md` + `tests/optin_catalog.rs`. |
| 32 | `minsvg serve` (`--features serve`) loopback; `minsvg-mcp` (`--features mcp`) stdio. |
| 33 | `tests/landmines.rs` (9). |
| 34 | All requested suites green — see final report. |
| 35 | `leftover_svgo_410_builtin_plugin_ids_is_zero` — leftover **0**. |
| 37 | Dropped unused direct `indexmap` (lockfile still pins 2.11.4 for MSRV 1.83). Dropped oxipng `parallel` / rayon. Default release `minsvg` **1,991,736** bytes (was 2,058,888). Kept `serde_json`; no `serde_yaml`. `serve` / `mcp` stay optional. |
| 38 | `Cargo.toml` `default = []`; `mcp` / `serve` optional; `minsvg-mcp` `required-features = ["mcp"]`. |
| 39 | Default release `minsvg` **2,058,888** bytes (budget 2.5 MiB). |
| 40 | `npm/pack-check.mjs`: packed **3959** / unpacked **11123** (5 files). |
