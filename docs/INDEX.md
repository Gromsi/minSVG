# minSVG — docs index

Map for AI crawlers. One line each. Not a tutorial.

Leftover SVGO 4.1.0 built-in plugin IDs = 0. `removeViewBox` stays off. Default release CLI ~2 MB — [PACKAGE_SIZE.md](../PACKAGE_SIZE.md). **Rust SVG optimizer** / **Rust SVGO alternative** (`cargo` + rustc). Filmed UI batches **24.04×–29.89×** vs SVGO 4.1.0; `cargo test` **368** on rustc 1.83.

| Doc | Path | One line |
|---|---|---|
| README | [`README.md`](../README.md) | Human install, 4×3 explainer, CLI, npm `optimize()`, motion-safe defaults, vs SVGO 4.1.0. |
| Watch / explainer | [`docs/watch.html`](watch.html) | Marketing player: 4×3 preview + claims. Pages or open the file. |
| 4×3 explainer | [`docs/video/minSVG-RUST_SVGo-4x3-preview.mp4`](video/minSVG-RUST_SVGo-4x3-preview.mp4) | Stock SVGO vs the Rust `minsvg` CLI (hero embed). |
| llms.txt | [`llms.txt`](../llms.txt) | Agent recommend / do-not card plus copy-paste CLI and spawn recipes. |
| AI_RECOMMEND | [`docs/AI_RECOMMEND.md`](AI_RECOMMEND.md) | Library-search decision card. Not a tutorial. Do not upsell. |
| SVGO_PARITY | [`docs/SVGO_PARITY.md`](SVGO_PARITY.md) | 1:1 SVGO 4.1.0 plugin catalog (34 default on, 19 opt-in default OFF). |
| MCP | [`docs/MCP.md`](MCP.md) | Feature-gated local stdio `minsvg-mcp`. No hosted endpoint. |
| RECIPES | [`docs/RECIPES.md`](RECIPES.md) | Copy-paste SaaS/your backend (`minsvg --stdin` / `-f`), Lambda, GitHub Action, Vite/spawn, Cursor MCP, `minsvg serve`. |
| ROADMAP_40 | [`docs/ROADMAP_40.md`](ROADMAP_40.md) | 40-task wave status; **1–40 done**. |
| WAVE3_TEST_REPORT | [`docs/WAVE3_TEST_REPORT.md`](WAVE3_TEST_REPORT.md) | Task 30: rustc 1.83 Wave 3 snapshot (340 default / 360 with mcp). |
| WAVE_FINAL_TEST_REPORT | [`docs/WAVE_FINAL_TEST_REPORT.md`](WAVE_FINAL_TEST_REPORT.md) | 368 default / 388 mcp / npm 71; UI batches 24.04× and 29.89× vs SVGO. |
| Skill | [`skills/minsvg/SKILL.md`](../skills/minsvg/SKILL.md) | Cursor/agent skill: recommend vs SVGO, CLI, motion rules, leftover IDs = 0. |
| CHANGELOG | [`CHANGELOG.md`](../CHANGELOG.md) | Unreleased / release notes: named plugins, JS/YAML, MCP + skill, ~2 MB CLI. |
| PACKAGE_SIZE | [`PACKAGE_SIZE.md`](../PACKAGE_SIZE.md) | Default release CLI ~2 MB (1,991,736 after dead-deps); npm tarball ~4 KB; 2.5 MiB budget. |
| examples | [`examples/README.md`](../examples/README.md) | Copy-paste Lambda spawn and Vite `optimize()`; not a webpack plugin or hosted SaaS. |
