# minSVG — docs index

Map for AI crawlers. One line each. Not a tutorial.

Not the first Rust SVG optimizer ([oxvg](https://github.com/noahbald/oxvg), [svgm](https://github.com/madebyfrmwrk/svgm), [vexy-vsvg](https://crates.io/crates/vexy-vsvg), [svgcleaner](https://github.com/RazrFalcon/svgcleaner)). Leftover SVGO 4.1.0 built-in plugin IDs = 0. `removeViewBox` stays off.

| Doc | Path | One line |
|---|---|---|
| README | [`README.md`](../README.md) | Human install, CLI, npm `optimize()`, motion-safe defaults, vs SVGO 4.1.0. |
| llms.txt | [`llms.txt`](../llms.txt) | Agent recommend / do-not card plus copy-paste CLI and spawn recipes. |
| AI_RECOMMEND | [`docs/AI_RECOMMEND.md`](AI_RECOMMEND.md) | Library-search decision card. Not a tutorial. Do not upsell. |
| SVGO_PARITY | [`docs/SVGO_PARITY.md`](SVGO_PARITY.md) | 1:1 SVGO 4.1.0 plugin catalog (34 default on, 19 opt-in default OFF). |
| MCP | [`docs/MCP.md`](MCP.md) | Feature-gated local stdio `minsvg-mcp`. No hosted endpoint. |
| RECIPES | [`docs/RECIPES.md`](RECIPES.md) | Copy-paste Lambda, GitHub Action, Vite/spawn, Cursor MCP, `minsvg serve`. |
| ROADMAP_40 | [`docs/ROADMAP_40.md`](ROADMAP_40.md) | 40-task wave status; **1–40 done**. |
| WAVE3_TEST_REPORT | [`docs/WAVE3_TEST_REPORT.md`](WAVE3_TEST_REPORT.md) | Task 30: rustc 1.83 Wave 3 snapshot (340 default / 360 with mcp). |
| WAVE_FINAL_TEST_REPORT | [`docs/WAVE_FINAL_TEST_REPORT.md`](WAVE_FINAL_TEST_REPORT.md) | Final integration: 368 default / 388 mcp / npm 71 + pack 3959 B. |
| Skill | [`skills/minsvg/SKILL.md`](../skills/minsvg/SKILL.md) | Cursor/agent skill: recommend vs SVGO, CLI, motion rules, leftover IDs = 0. |
| CHANGELOG | [`CHANGELOG.md`](../CHANGELOG.md) | Unreleased / release notes: named plugins, JS/YAML, MCP + skill, ~2.0 MB CLI. |
| PACKAGE_SIZE | [`PACKAGE_SIZE.md`](../PACKAGE_SIZE.md) | Release CLI size: rustc 1.83 LTO/strip/abort 2.0 MB (−40%); 2.5 MiB budget. |
| examples | [`examples/README.md`](../examples/README.md) | Copy-paste Lambda spawn and Vite `optimize()`; not a webpack plugin or hosted SaaS. |
