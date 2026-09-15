# minSVG vs SVGO 4.1.0 (real-life corpus)

In-process [`minsvg::optimize`] vs pinned **SVGO 4.1.0** (`svgo-rust/adapters/svgo_runner`, `multipass: true` in `run.mjs`).

This is **not** an oxvg, svgm, or vexy-vsvg claim. A smaller byte count is **not more correct**.

**Wall time:** minSVG is library-only (`std::time::Instant` around `optimize`, one warmup). SVGO is a fresh `node run.mjs` spawn — Node startup is included. Times are from this machine; they drift.

**Dest-count:** destination points in `d` (M/L/H/V/C/S/Q/T/A). The ocean-triangle / rust-gear guard. No resvg (MSRV 1.83).

Files are **read-only** from sibling `svgo-rust/corpus/`. The table keeps the three illustrated files and adds small icons (`circle_icon.svg`, `heart.svg`, `star.svg`, `check.svg`). `fun/00-driving-pov.svg` (~4 MB, 162-frame WebP movie) and `large/corvus-corax-iucn.svg` (~25 MB raven map) are **not** copied into this repo and are **not** in this table.

Refresh after an intentional optimizer change:

```
UPDATE_GOLDEN=1 cargo test --test parity_reallife --offline -- --nocapture
```

- SVGO: 4.1.0 (`node adapters/svgo_runner/run.mjs --input --output`)
- minSVG: `minsvg::optimize` (in-process)

| file | in | minSVG B | SVGO B | minSVG ms | SVGO ms | dests in/minSVG/SVGO | notes |
|---|---:|---:|---:|---:|---:|---:|---|
| `fun/capybara.svg` | 2873 | 1617 | 1592 | 1.74 | 146.8 | 13/21/21 | hat ticks on-canvas; unused wrap dropped |
| `logos/rust.svg` | 4244 | 4244 | 3300 | 0.729 | 129.9 | 201/201/201 | title+path kept; dest-count held |
| `animation/capybara-hot-spring.svg` | 7282 | 5378 | 4942 | 3.44 | 152.8 | 10/10/20 | url(#poolFill) kept; unused wrap kept (motion) |
| `circle_icon.svg` | 343 | 164 | 164 | 0.195 | 150.7 | 0/0/0 | circle kept; fill #36f |
| `heart.svg` | 286 | 186 | 182 | 0.147 | 123.4 | 5/5/5 | path kept; unused wrap dropped |
| `star.svg` | 354 | 209 | 197 | 0.198 | 138.2 | 0/0/10 | polygon kept; unused wrap dropped |
| `check.svg` | 586 | 259 | 259 | 0.312 | 115.1 | 3/3/3 | badge circle; unused grad dropped |

Negative `minSVG B − SVGO B` is a compression badge, not a correctness claim. On `animation/capybara-hot-spring.svg`, minSVG keeps `url(#poolFill)` / motion ids (animation-aware). Stock SVGO 4.1.0 may inline CSS selectors and drop unused wraps; that is smaller, not more correct.
