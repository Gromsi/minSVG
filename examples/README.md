# minSVG examples

Copy these into **your** app. They are not a webpack plugin and not a hosted SaaS.

SaaS / your backend: `cargo install --git https://github.com/Gromsi/minSVG --locked`, copy the ~2 MB `minsvg` into Docker/Lambda, then `minsvg --stdin` or `-f`. Do not expose `minsvg serve` as the public edge.

The npm helper `optimize()` **spawns** the Rust `minsvg` binary (`PATH` or `MINSVG_BIN`). Put that binary on PATH first (`cargo install`). Full recipes (SaaS, CI, MCP, `minsvg serve`): [`docs/RECIPES.md`](../docs/RECIPES.md).

| Snippet | Path | What it does |
|---|---|---|
| Lambda **spawn** | [`lambda/index.mjs`](lambda/index.mjs) | Node handler: `spawnSync(minsvg, ['--stdin'])`. Attach to **your** API Gateway or Function URL. |
| Vite **`optimize()`** | [`vite/vite.config.js`](vite/vite.config.js) | Plugin **you** own: `import { optimize } from 'minsvg'` on emitted `.svg` assets. `vite build` only. |

## Lambda spawn

Attach [`lambda/index.mjs`](lambda/index.mjs) yourself. We do not host this and do not open a public URL.

```js
import { spawnSync } from 'node:child_process'

const result = spawnSync(process.env.MINSVG_BIN || 'minsvg', ['--stdin'], {
  input: svgString,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
})
```

JSON `{ "svg" }` → `{ "data" }`. Raw SVG body → `image/svg+xml`. Linux binary on PATH (layer, zip, or an image **you** build). The repo `Dockerfile` is ECS / Cloud Run `serve`, not a Lambda image.

## Vite `optimize()`

Copy [`vite/vite.config.js`](vite/vite.config.js). It is **not** `vite-plugin-minsvg`, **not** `vite-plugin-svgo`, **not** webpack / `svgo-loader`.

```js
import { optimize } from 'minsvg'

const result = optimize(source, options)
if (result.error) throw result.modernError ?? new Error(result.error)
item.source = result.data
```

Prefer `minsvg -f src/assets --recursive` in `prebuild` so Vite’s SVG-as-URL pipeline stays untouched. The `generateBundle` sketch runs at `vite build` only.
