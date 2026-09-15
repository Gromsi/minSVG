# SVGO 4.1.0 ↔ minSVG plugin catalog

1:1 ID list against [svgo@4.1.0](https://github.com/svg/svgo/releases/tag/v4.1.0) `plugins/preset-default.js` + `lib/builtin.js`. The [svgo.dev preset-default page](https://svgo.dev/docs/preset-default/) omits `removeDeprecatedAttrs` (4.1.0 runs it after `removeComments`). This file uses the **package source**: **34** default plugins + **19** opt-in plugins + the `preset-default` bundle.

Names follow public SVGO plugin IDs so a migrate checklist is possible. Implementations are **clean-room conservative subsets** (public contracts only).

## Honesty

- **Not the first Rust SVG optimizer.** [oxvg](https://github.com/noahbald/oxvg), [svgm](https://github.com/madebyfrmwrk/svgm), [vexy-vsvg](https://crates.io/crates/vexy-vsvg), and [svgcleaner](https://github.com/RazrFalcon/svgcleaner) already exist.
- **Not oxvg** (and not a port of SVGO / oxvg / svgm / vexy source). We studied public docs and architecture only.
- **v1 is not full SVGO parity.** Every 4.1.0 plugin **ID** is named and callable. Passes are conservative subsets — `have` ≠ byte-identical output. Path-heavy files often stay **larger** than SVGO: default `convertPathData` refuses `floatPrecision: 3`.
- **No SMIL frame wipe.** Stock SVGO `removeHiddenElems` can delete `visibility="hidden"` frames that an `<animate>` later shows. We do not.
- **`url(#Id)` case is kept.** Fragment identifiers are case-sensitive (`url(#poolFill)` stays). No ID minify / case-fold on the default path.
- **`removeViewBox` stays off.** Same policy as SVGO 4 defaults. It is implemented as an opt-in and is **not** in the default pipeline.

The filmed comparison machine is [Gromsi/svgo-rust](https://github.com/Gromsi/svgo-rust), not this crate. That sibling can enable a 3% visual-budget path rewrite. **This crate does not.**

---

## How to read the columns

| Value | Meaning |
|---|---|
| **yes (subset)** | Named pass exists. Usually a **conservative subset** of SVGO — see Notes. |
| **yes** | Named pass exists and matches the public contract closely enough that leftover is params / host policy, not a missing rewrite. |
| **added** | minSVG-only. Not an SVGO plugin ID. |
| **no** | SVGO 4.1.0 has this ID. minSVG does not implement it (a sliver elsewhere does not count). |
| **on** | Runs on `Config::default()` / `minsvg in.svg` (unless animation-aware skips it). |
| **off** | Stays out of `DEFAULT_PLUGIN_NAMES`. Enable with `--plugin NAME` / `Config::enable_plugins` / JS `{ name, params }`. |

`have` in older notes = **yes (subset)** here. **Implemented** is about the ID being wired. **Default** is about whether that ID runs without `--plugin`.

---

## Snapshot

| Bucket | Count |
|---|---:|
| SVGO `preset-default` plugin IDs | 34 |
| of those, **implemented** | **34** |
| of those, **missing** | **0** |
| of those, **default on** | 34 (several skip on motion docs) |
| SVGO built-ins **not** in `preset-default` | 19 opt-in plugins + the `preset-default` bundle |
| of those 19 opt-in IDs, **implemented** | **19** |
| of those 19, **missing** | **0** |
| of those 19, **default on** | **0** (`removeViewBox` included — stays **off**) |
| minSVG **added** named passes | 2 (`collapseWhitespace`, `recompressEmbeddedRasters`) — both default **on** |

`minsvg plugins` prints `DEFAULT_PLUGIN_NAMES` (36 = 34 SVGO IDs + 2 added), then `OPTIN_PLUGIN_NAMES` (19, default **off**), then `MOTION_SKIP_PLUGINS`.

---

## Pipeline / product features

| Feature | SVGO 4.1.0 | minSVG | Notes |
|---|---|---|---|
| Default plugin set | `preset-default` (34) | `DEFAULT_PLUGIN_NAMES` (36: 34 SVGO IDs + 2 added) | Run order is **not** SVGO order (`removeEmptyAttrs` / `removeDesc` / colors vs numerics differ). |
| `preset-default` bundle | **yes** (`lib/builtin.js`) | mapped, not a pass ID | Config / JS `name: 'preset-default'` means “the default set” + `overrides`. It is not listed in `DEFAULT_PLUGIN_NAMES`. |
| `plugins: […]` / `overrides` | **yes** | **yes** (skip / enable / some params) | `{ name, active: false }` and `preset-default` overrides → `--skip`. String name / `{ name, active: true }` → `--plugin`. `{ name, params }` → `--plugin` + `--param name=<json>`. Last name wins; `--skip` beats `--plugin` on the same id. Does **not** replace the default pipeline. No custom JS plugins. |
| Per-plugin `params` | **yes** | **yes** on opt-ins that declare them | Wired JSON: `prefixIds`, `cleanupListOfValues`, `convertStyleToAttrs`, `addAttributesToSVGElement`, `addClassesToSVGElement`, `removeAttrs`, `removeAttributesBySelector`, `removeElementsByAttr`. Default-set plugins: skip/active only (no SVGO `convertPathData.applyTransforms`, etc.). |
| `multipass` | **yes** (default `false`) | **yes** (`--multipass`, default `false`) | Re-run until serialized size is stable, max 10. Same AST; no extra path-geometry rewrite. |
| Fixed-point dirty loop | n/a (string multipass) | **no** | TODO in `plugins.rs` (svgm-style). |
| `js2svg.pretty` / indent | **yes** | **yes** (`--pretty` / `--indent`) | Plugin loop stays compact. Pretty is a final emit (default indent 4). |
| `datauri` | **yes** | **yes** (`--datauri`) | `base64` / `enc` / `unenc`. |
| `floatPrecision` | **yes** (paths default 3) | **yes** (`--precision`) | Omit for lossless path `d` + numeric default 3. Motion still skips path rewrite. Dest-count / digit-glue / bbox guards stay. |
| Legal comments (`<!--! … -->`) | keep | **yes** | Same public contract as SVGO / oxvg / vexy. |
| Animation-aware skip plan | **no** (stock runs everything) | **added** | See [Motion](#motion-aware-defaults). |
| `--extra` sibling `#id` scan | **no** | **added** | JS/TS/CSS texts via CLI / `Config.extra_sources`. No automatic workspace walk. |
| SMIL clock rewrite (`id.end+.33s`) | `prefixIds` is incomplete ([#848](https://github.com/svg/svgo/issues/848) / [#2073](https://github.com/svg/svgo/issues/2073) / [#2207](https://github.com/svg/svgo/issues/2207)) | **yes** when `prefixIds` is on | `rewrite_smil_clock_value`. Unused on the default path (we do not prefix IDs unless asked). |
| HTTP `optimize` | **no** (CLI / Node API) | **added** | `minsvg serve` (`--features serve`). You start it. Not a hosted CDN. |

---

## `preset-default` (34 IDs, 4.1.0 source order)

All **implemented**. All **default on**. Several are **subsets**. Motion-sensitive docs still skip the IDs in `MOTION_SKIP_PLUGINS`.

| # | Plugin | Implemented | Default | Notes |
|---:|---|---|---|---|
| 1 | `removeDoctype` | yes | on | Drop the DTD. |
| 2 | `removeXMLProcInst` | yes | on | Drop `<?xml …?>` only. Other PIs stay. |
| 3 | `removeComments` | yes | on | Keeps legal `!` comments. |
| 4 | `removeDeprecatedAttrs` | yes (subset) | on | `removeAny: false`: `clip` / `kerning` / `color-profile`; `enable-background` when the doc has no `<filter>`; root `version`. |
| 5 | `removeMetadata` | yes | on | |
| 6 | `removeEditorsNSData` | yes | on | Public editor prefixes (Inkscape / Illustrator / Figma / …). |
| 7 | `cleanupAttrs` | yes (subset) | on | Squeeze whitespace in **values** only. |
| 8 | `mergeStyles` | yes (subset) | on | Fold mergeable `<style>` sheets into the first one. Selector spelling and `url(#Id)` case stay. Skip `<foreignObject>` and non-CSS `type`. Empty sheets drop. `media` wraps as `@media …{…}`. Does **not** inline rules or drop IDs. |
| 9 | `inlineStyles` | yes (subset) | on | Icon/static subset (`onlyMatchedOnce` / `removeMatchedSelectors`). **Never** inlines `#id` / `type#id` (do not copy SVGO’s onsen ID strip). Never deletes `id` / `xml:id`. Shared class / type selectors stay. `@keyframes` / `@media` / animation / transition rules stay in the sheet. Skipped on motion. |
| 10 | `minifyStyles` | yes (subset) | on | Comments + safe whitespace + empty decls/rules + a few SVG default decls. **Not** CSSO / LightningCSS. No unused-selector prune. Non-zero CSS `px` stays. Also runs a cascade-safe `convertStyleToAttrs` sliver (no `<style>`, no `class`). |
| 11 | `cleanupIds` | yes (subset) | on | Drop unused `id` / `xml:id` only. **No minify / rename.** Aborts if `<script>` / `on*` are present. Skipped on motion. |
| 12 | `removeUselessDefs` | yes (subset) | on | Unreferenced paint / clip / filter in `<defs>`. Keeps `<style>` / `<script>` and `class` targets. Motion keeps unreferenced ids. |
| 13 | `cleanupNumericValues` | yes (subset) | on | Precision 3 on **presentation** attrs. Does **not** touch path `d` or SMIL timing (`begin` / `end` / `dur` / `values` / …). |
| 14 | `convertColors` | yes (subset) | on | `names2hex` / `rgb2hex` / `shorthex` / `shortname` / lower. `currentColor` **off**. RGBA/HSLA with alpha ≠ 1 stays. |
| 15 | `removeUnknownsAndDefaults` | yes (subset) | on | Default presentation attrs only (`unknownContent` / `unknownAttrs` stay **off**). **Never** drops `role` (incl. `role="img"`), `viewBox`, `xmlns` / `xmlns:*`, `aria-*`, `data-*`, or `<title>`. Never deletes elements. SMIL-targeted attrs stay. With a `<style>` sheet, inheritable defaults stay. |
| 16 | `removeNonInheritableGroupAttrs` | yes (subset) | on | Dead-on-`<g>` primitives only (`flood-color`, `stop-color`, …). Inheritable paint / fonts stay. `opacity` / `filter` / `mask` / `clip-path` / `display` stay. Attributes only — not `style=""`. |
| 17 | `removeUselessStrokeAndFill` | yes (subset) | on | Inheritance-aware; never mutates inheriting containers; `removeNone` stays false. No computed-style engine. |
| 18 | `cleanupEnableBackground` | yes (subset) | on | Attrs + inline `style` (not `<style>` sheets). Drop when the doc has no `<filter>`; shorten matching `new 0 0 W H` on `<svg>` / `<mask>` / `<pattern>`. |
| 19 | `removeHiddenElems` | yes (subset) | on | **Policy diverge.** See [SMIL](#removehiddenelems-we-do-not-copy-smil-deletion). |
| 20 | `removeEmptyText` | yes | on | Empty `<text>` / `<tspan>`, dangling `<tref>`. |
| 21 | `convertShapeToPath` | yes (subset) | on | `line` / `polyline` / axis-aligned `rect` (no `rx`/`ry`). No `polygon`, circle, ellipse (`convertArcs` stays off). Skipped on motion. |
| 22 | `convertEllipseToCircle` | yes (subset) | on | `rx == ry` only. Skips SMIL-animated radii. |
| 23 | `moveElemsAttrsToGroup` | yes (subset) | on | Hoist identical inheritable presentation attrs from **two or more** content children. No `transform` / `opacity` / `style` hoist. Skip filter / mask / clip groups and motion-id groups. |
| 24 | `moveGroupAttrsToElems` | yes (subset) | on | Copy `<g transform>` onto transformable children (string concat), then drop it on the group. No matrix bake. Skip filter / mask / clip / motion-id / SMIL-`transform` groups. |
| 25 | `collapseGroups` | yes (subset) | on | Unwrap **attrless** `<g>` only. Runs after the movers. Skipped on motion. |
| 26 | `convertPathData` | yes (subset) | on | Rel/abs, H/V/S/T/Z, leading zeros, omit separators before `-` / commands. **No** default `floatPrecision: 3`, arc conversion, or overlapping-subpath merge. Refuses digit-glue and dest-count collapse. Skips SMIL-animated `d`. Skipped on motion. |
| 27 | `convertTransform` | yes (subset) | on | Collapse `translate` / `scale` / `matrix` to a shorter list. Drops identity. **No** bake into path `d`. Skips `animateTransform` / `animateMotion` targets. Skipped on motion. |
| 28 | `removeEmptyAttrs` | yes | on | Keeps empty SVG conditional-processing trio. |
| 29 | `removeEmptyContainers` | yes (subset) | on | Never drops `<svg>`. Keeps referenced ids; motion can keep empty marked groups. |
| 30 | `mergePaths` | yes (subset) | on | Sibling **stroke-only** (`fill="none"`) paths with identical paint. Skips `id`, markers, filled/classed shapes, SMIL children. Skipped on motion. |
| 31 | `removeUnusedNS` | yes | on | Whole-tree prefix use, then drop unused `xmlns:*` on **every** element (SVGO docs: outermost `<svg>` only). |
| 32 | `sortAttrs` | yes | on | Gzip/brotli only; not visual. `xmlnsOrder: front`, then the public `order` list; leftovers alphabetical. `stroke-*` / `marker-*` follow their prefix key. |
| 33 | `sortDefsChildren` | yes | on | Frequency (desc), name length, name, then `id`. Comments / PIs stay ahead of elements. |
| 34 | `removeDesc` | yes (subset) | on | `removeAny: false` — empty or editor-attribution only. |

---

## Opt-in built-ins (19 IDs, default **off**)

SVGO 4.1.0 ships these in `lib/builtin.js` but **not** in `preset-default`. minSVG implements every ID. **None** run on `Config::default()`. Several stay off on purpose for icons / a11y / standalone files — enable only when you mean it.

`removeViewBox` is in this list. It is **implemented** and **default off**. Do not document it as a default.

| # | Plugin | Implemented | Default | Notes |
|---:|---|---|---|---|
| 1 | `removeViewBox` | yes (subset) | **off** | Drop `viewBox` when it is `0 0 width height` on the outermost `<svg>` plus `<pattern>` / `<symbol>`. Nested `<svg>` viewports stay. Breaks scaling / clips icons. **Stay off.** |
| 2 | `removeDimensions` | yes | **off** | Drop outermost `<svg>` `width`/`height`. If `viewBox` is missing and both dimensions are plain numbers, synthesize `viewBox="0 0 W H"` first. Useful only for some responsive hosts. |
| 3 | `removeXMLNS` | yes | **off** | Drop the default `xmlns` on the outermost `<svg>` only. `xmlns:*` and `role` stay. Breaks standalone files and `<img>`. HTML-inline only. |
| 4 | `removeXlink` | yes (subset) | **off** | SVG 2 migrate: `*:href` → `href`, `*:show` → `target`, `*:title` → `<title>`. `includeLegacy: false` only (not a `--param`). Legacy hosts (`cursor`, `filter`, `font-face-uri`, `glyphRef`, `tref`) keep XLink. Unused XLink xmlns drops when nothing remains. |
| 5 | `removeTitle` | yes | **off** | Detach every `<title>`. Left off in SVGO 4 for a11y. SMIL / graphics siblings and `role` stay. **Stay off** unless you accept the a11y trade-off. |
| 6 | `prefixIds` | yes (subset) | **off** | Prefix `id` / class + rewrite `url(#Id)` / `#` href / aria id lists. **`url(#Id)` case kept** (`url(` match is case-insensitive; fragment is not folded). Rewrites SMIL `begin`/`end` syncbases (stock SVGO often does not — #848 / #2073 / #2207). Params: `prefix`, `delim` (default `__`), `prefixIds`, `prefixClassNames`. Default prefix: source filename (`.` / space → `_`) or `prefix`. Sprite collision fix — not a default. |
| 7 | `removeRasterImages` | yes | **off** | Drop `<image>` whose `href` / `xlink:href` is a raster file or `data:image/…` (not `image/svg+xml`). Visual data loss. Default pipeline **recompresses** instead (`recompressEmbeddedRasters`). |
| 8 | `removeOffCanvasPaths` | yes (subset) | **off** | Drop `<path>` whose control-polygon bbox sits clearly outside the nearest `<svg viewBox>` (+ pad; stroke inflated). Skips the whole pass on motion docs when animation-aware. Always skip transform (self/ancestor), SMIL children, markers, filter/mask, arcs, unparseable `d`, and paths in `defs` / `symbol` / `clipPath` / `mask` / `pattern` / … Needs a trustworthy `viewBox`. |
| 9 | `removeScripts` | yes | **off** | Drop `<script>` and `on*` event attributes (v4 name; was `removeScriptElement`). The element that carried `onclick` stays. Sanitizer, not a minifier. Default path **keeps** `<script>` / `on*` (they trip animation-aware skips). Not an XSS filter. |
| 10 | `removeStyleElement` | yes (subset) | **off** | Drop `<style>` elements. Inline `style=""` stays. Skips the pass on motion-sensitive docs when animation-aware (`@keyframes` / SMIL / script / `on*`). |
| 11 | `reusePaths` | yes (subset) | **off** | Identical path `d` (≥2) → one `<path id>` in `<defs>` + `<use href="#id">`. Paint / `transform` / `style` stay on the `<use>`. Keys on `d` only (SVGO also keys fill/stroke). Skips motion docs; protected SMIL / `url(#)` / CSS `#id` / `<use>` targets stay `<path>`. Changes DOM / CSS selectors. |
| 12 | `convertOneStopGradients` | yes (subset) | **off** | Single-stop `linearGradient` / `radialGradient` → solid paint; drop the definition; drop empty `<defs>`; drop unused `xmlns:xlink`. Follows `href` inherit. Skips motion docs; SMIL-named gradient ids stay. Interpolation risk — opt-in. |
| 13 | `addAttributesToSVGElement` | yes | **off** | Injector. Params: `attribute` / `attributes` (string, object, or array). Writes **missing** keys only on each outermost `<svg>`. Nested `<svg>` ignored. Empty params = no-op. |
| 14 | `addClassesToSVGElement` | yes | **off** | Injector. Params: `className` / `classNames` (`classNames` wins when non-empty). Appends tokens; duplicates drop. Outermost `<svg>` only. Empty params = no-op. |
| 15 | `removeAttrs` | yes (subset) | **off** | User-configured deletion. Params: `attrs` globs (`fill`, `stroke-*`, `path:fill:none`), `elemSeparator` (default `:`), `preserveCurrentColor`. `*` / `?` globs, not regex. `xml:` / `xlink:` / `xmlns:` stay one attribute when the separator is `:`. Empty `attrs` = no-op. |
| 16 | `removeAttributesBySelector` | yes (subset) | **off** | Simple `tag` / `#id` / `.class` / comma lists (`circle, rect`). No combinators, attribute selectors, or pseudos — those fail closed. Params: `selector`+`attributes` or `selectors: [{ selector, attributes }]`. |
| 17 | `removeElementsByAttr` | yes | **off** | Drop elements by `id` / `class` / `data` (`foo`, `data-foo`, `foo=bar`, `data-*`). Outermost `<svg>` is never removed. Empty params = no-op. |
| 18 | `cleanupListOfValues` | yes (subset) | **off** | Round `points` (polyline/polygon), `enable-background`, `viewBox`, `stroke-dasharray`, list-valued `x` / `y` / `dx` / `dy`. Skips SMIL elements and path `d`. Params: `floatPrecision` (default 3), `leadingZero`, `defaultPx`, `convertToPx`. |
| 19 | `convertStyleToAttrs` | yes (subset) | **off** | Promote `style=""` presentation decls → attributes. Leaves `transform` / `transform-origin` / `font-family`, `var()`/`calc()`, CSS-wide keywords, and SMIL `attributeName` targets in CSS. Param: `keepImportant`. A **cascade-safe sliver** already runs inside default `minifyStyles` (no `<style>`, no `class`) — this named pass is the aggressive one. |

Enable: `minsvg in.svg --plugin removeTitle` or JS `optimize(svg, { plugins: [{ name: 'removeTitle' }] })`. `--skip` wins over `--plugin` on the same id. `--param name=json` sets JSON but does **not** enable a default-off pass by itself.

---

## minSVG-only named passes (**added**)

| Pass | SVGO 4.1.0 | Implemented | Default | Notes |
|---|---|---|---|---|
| `collapseWhitespace` | serializer (`js2svg`), not a plugin ID | added | on | Drop ignorable whitespace text nodes. Honors inherited `xml:space`. |
| `recompressEmbeddedRasters` | **no** (opt-in `removeRasterImages` **deletes** rasters) | added | on | oxipng PNG / JPEG metadata strip + `<image>` wrapper minify. **Runs on motion docs.** Never drops `href` / `xlink:href`. Leaves already-tight lossy WebP alone. |

These two sit in `DEFAULT_PLUGIN_NAMES` after `removeUnusedNS`.

Not a plugin ID: `numericRounding` appears only in `MOTION_SKIP_PLUGINS` as a reserved planner name (path-`d` precision). This crate does not ship that rewrite.

---

## `removeHiddenElems`: we do **not** copy SMIL deletion

Stock SVGO 4.1.0 `removeHiddenElems` (all checks on by default) treats `visibility="hidden"` / `display="none"` / `opacity:0` / zero-size geometry as dead. On a SMIL visibility slideshow that is **wrong**: frames start hidden and an `<animate attributeName="visibility">` reveals them. Multipass can delete every frame except a ~40 ms poster, so the preview looks empty. That is deletion of the movie, not compression.

minSVG **refuses that behavior**:

1. **Default:** the whole pass is in `MOTION_SKIP_PLUGINS`. SMIL / CSS `@keyframes` / script / events / `--extra` `#id` refs skip it.
2. **When the pass does run** (static icons, or `--no-animation-aware`): only `display="none"` and `visibility="hidden|collapse"` (attribute or `style=""`). Referenced `id`s stay. **`opacity:0` is not deleted** (map `.circle` classes use it). Zero-size / empty-`d` / empty-`points` checks are **not implemented**.
3. **`recompressEmbeddedRasters` never deletes** `visibility="hidden"` artwork or `data:` `href` / `xlink:href`. A tiny SMIL loop must not freeze PNG/JPEG recompress.

`--no-animation-aware` runs the display/visibility subset on motion docs. That is still **not** SVGO’s full plugin, but it can drop hidden SMIL frames. Leave animation-aware on for movies.

---

## `url(#Id)` case

SVG fragment IDs are case-sensitive. Default minify **does not** fold `url(#poolFill)` to `url(#poolfill)` or rename the id.

- `mergeStyles` / `minifyStyles` / `convertStyleToAttrs` compact `url( '#poolFill' )` → `url(#poolFill)` without changing the fragment.
- `cleanupIds` only **drops unused** ids; it does not minify or lowercase remaining ones.
- Opt-in `prefixIds` prefixes the fragment (`url(#icon__poolFill)`) and still does not lowercase it.

---

## Motion-aware defaults

`Config.animation_aware` is **true** unless `--no-animation-aware`. Detection: SMIL tags, CSS `@keyframes`, `<script>` / `on*`, extra-source `#id` / `getElementById` that match nodes in the SVG.

Skipped on those documents (`MOTION_SKIP_PLUGINS`):

```
cleanupIds
inlineStyles
mergePaths
convertShapeToPath
collapseGroups
removeHiddenElems
numericRounding
convertPathData
convertTransform
```

`numericRounding` is a planner name (path-`d` precision). This crate does not ship that rewrite; the skip is reserved so a later budget pass cannot run on motion.

Group movers still **run** on motion docs, but they skip groups whose `id` is a motion hook. `mergeStyles` / sorts / `removeUnknownsAndDefaults` also run (they do not rename IDs or bake transforms).

Still run on motion: strip/minify that does not rename IDs or rewrite animated `d`, plus **`recompressEmbeddedRasters`**.

Opt-ins that also no-op on motion when animation-aware: `removeOffCanvasPaths`, `removeStyleElement`, `reusePaths`, `convertOneStopGradients`.

---

## Still **missing** (subset gaps — not missing IDs)

Every 4.1.0 plugin **ID** is wired (34 default + 19 opt-in). Leftovers are deeper SVGO behavior:

| Gap | Why it is still out |
|---|---|
| `convertTransform` bake-into-path | Attr collapse is in; applying the matrix to `d` is still out. |
| Full `minifyStyles` (LightningCSS / CSSO) | Unused-selector prune can break sprites. Non-zero CSS `px` stays on purpose. |
| `inlineStyles` `#id` / multi-match inlining | Intentionally incomplete so onsen / sprite hooks survive. |
| `removeUnknownsAndDefaults` `unknownContent` / `unknownAttrs` | Can strip SVG2 / tool-specific bits. Left **off**. |
| `convertPathData` precision 3 / arcs / applyTransforms | Highest SVGO size win; unsafe for icons. Not in this crate (no visual-budget flag). |
| Full `removeHiddenElems` zero-size / `opacity:0` | Intentionally incomplete; SMIL frames must survive. |
| `removeXlink` `includeLegacy: true` via `--param` | Function exists; the named `--plugin` path stays `false`. |
| `removeAttributesBySelector` full CSS engine | Simple tag / `#id` / `.class` only. |
| `reusePaths` paint-keyed templates | We key on `d` only. |
| Multipass / dirty-flag loop | Merge-wire concern, not a plugin. |
| Raster goldens (resvg) | Correctness oracle before expanding geometry passes. |

**Never in our default:** `removeViewBox`, `removeTitle`, `removeXMLNS`, `removeRasterImages`, `removeScripts` as an optimizer, `prefixIds`.

---

## How to read `minsvg plugins`

```
minsvg plugins
minsvg plugins --json
```

Prints:

1. `DEFAULT_PLUGIN_NAMES` (wired / default **on**)
2. `OPTIN_PLUGIN_NAMES` (SVGO opt-in IDs / default **off**)
3. `MOTION_SKIP_PLUGINS`

JSON adds a one-line `convertPathData` contract (conservative lossless; optional `--precision`; dest-count/glue guards). MCP `minsvg_list_plugins` is the same catalog plus `param_hints`.
