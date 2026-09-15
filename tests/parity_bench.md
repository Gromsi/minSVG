# minSVG vs SVGO 4.1.0 (tiny fixtures)

In-process [`minsvg::optimize`] vs pinned **SVGO 4.1.0** (`svgo-rust/adapters/svgo_runner`, `multipass: true` in `run.mjs`).

This is **not** an oxvg, svgm, or vexy-vsvg claim. A smaller byte count is **not more correct**.

**Wall time:** minSVG is library-only (`std::time::Instant` around `optimize`, one warmup). SVGO is a fresh `node run.mjs` spawn — Node startup is included. Times are from this machine; they drift.

**Cheap pixel / dest-count:** 8×8 occupancy from circle / rect / path dest-polygon (even-odd). Dest-count is destination points in `d` — the ocean-triangle guard. No resvg (MSRV 1.83).

Fixtures are tiny on purpose. The 162-frame driving-POV movie and the 1.7MB world-map ocean are **not** in this table — `smil-visibility.svg` and `path.svg` are the stand-ins so CI stays small. Existing 162-frame / ocean tests are unchanged.

Refresh after an intentional optimizer change:

```
UPDATE_GOLDEN=1 cargo test --test parity_bench --offline -- --nocapture
```

- SVGO: 4.1.0 (`node adapters/svgo_runner/run.mjs --input --output`)
- minSVG: `minsvg::optimize` (in-process)

| fixture | in | minSVG B | SVGO B | minSVG ms | SVGO ms | dests in/minSVG/SVGO | occ Δ minSVG | occ Δ SVGO | notes |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `circle_icon.svg` | 343 | 164 | 164 | 0.165 | 118.6 | 0/0/0 | 0 | 0 | circle + #36f; occupancy match |
| `path.svg` | 386 | 209 | 159 | 0.244 | 132.1 | 5/5/5 | 0 | 0 | ocean cubic rect; dests≥4; no `014.1912` |
| `smil-visibility.svg` | 520 | 451 | 219 | 0.277 | 149.8 | 0/0/4 | 0 | 0 | minSVG keeps 2 frames; SVGO drops hidden |

Negative `minSVG B − SVGO B` is a compression badge, not a correctness claim. On `smil-visibility.svg`, stock SVGO 4.1.0 `removeHiddenElems` deletes the hidden SMIL frame (tiny stand-in for the 162-frame driving-POV bug). minSVG keeps both frames.
