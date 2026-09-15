# Changelog

All notable changes to this project are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The crate version is **0.1.0** and is not published on crates.io yet.

## [Unreleased]

Waves 1–5 of the installable MIT crate. Pipeline: parse → named plugins →
serialize.

v1 is **not** full SVGO parity. Every SVGO 4.1.0 built-in plugin **ID** is named
and callable; passes are conservative subsets. Path-heavy files often stay
larger (no default `convertPathData` at `floatPrecision: 3`).

Leftover SVGO 4.1.0 built-in plugin IDs = **0**: **34** `preset-default` IDs
default **on**, **19** opt-in IDs implemented and default **off**.
`removeViewBox` is implemented and stays **off** (same policy as SVGO 4
defaults). It is not in the default pipeline.

### Added

- **Catalog (waves 1, 4).** 34 default SVGO IDs plus two minSVG-only default
  passes (`collapseWhitespace`, `recompressEmbeddedRasters`). All 19 SVGO
  opt-ins exist (`removeViewBox`, `removeTitle`, `prefixIds`, `removeScripts`,
  …) and stay off unless `--plugin` / JS `{ name, params }` / a config
  `plugins` entry asks. `--param` alone does not enable a pass.
  `minsvg plugins` lists default vs opt-in vs motion skips.
- **JS `optimize()` (wave 2).** npm `minsvg` spawn wrapper: SVGO-shaped
  `{ data }`, plus `plugins` / `multipass` / `precision` mapped onto the CLI.
  Not wasm / napi. The Rust binary is not in the npm tarball (`PATH` /
  `MINSVG_BIN`).
- **YAML config (wave 2).** `--config` and cwd auto-load:
  `minsvg.config.toml` / `.json` / `.yaml` / `.yml`, then `svgo.config.json` /
  `.yaml` / `.yml`. YAML is a small subset (no `serde_yaml`). CLI flags
  override the file.
- **CLI flags (wave 2).** SVGO-shaped `--skip` / `--plugin` / `--param`,
  `--multipass`, `--precision`, `--pretty` / `--indent`, `--datauri`,
  `--quiet`, `--eol`, `--final-newline`, `--config`, `--extra`,
  `--no-animation-aware`, `--report`, stdin / `-f` / `--recursive`.
- **MCP, skill, llms (waves 1–3).** Feature-gated `minsvg-mcp`
  (`--features mcp`): `minsvg_optimize`, `minsvg_batch`,
  `minsvg_list_plugins`, `minsvg_recommend`. Agent skill
  `skills/minsvg/SKILL.md`. Broader card: `llms.txt`. `minsvg serve` stays
  `--features serve` (localhost default, no auth).
- **Tests (waves 2–4).** Catalog leftover=0, opt-in default-off (including
  `removeViewBox`), CLI e2e flags, JS/YAML/MCP, motion landmines, pixel/visual,
  path fidelity, real-life corpus, speed/size gates, npm spawn + pack check.
  Release size gate: `cargo test --release --test binary_size`.
- **Release size (wave 5).** Default `minsvg` (no `serve` / `mcp`) is
  **~2 MB** (`1,991,736` bytes after dead-deps; was `2,058,888`) after LTO +
  one CGU + strip + `panic=abort`. Budget 2.5 MiB. MCP and HTTP stay out of
  that binary. See `PACKAGE_SIZE.md`.

### Changed

- Animation-aware stays **on** by default: SMIL, CSS `@keyframes`,
  `<script>` / `on*`, and `--extra` `#id` refs skip ID rewrite and hidden-frame
  deletion. Raster recompress still runs.
- `removeViewBox` must stay off in defaults and docs. A leftover `viewBox`
  after default optimize is intended.

[Unreleased]: https://github.com/Gromsi/minSVG
