# minsvg

SVGO-shaped `optimize()` that **spawns** the Rust [`minsvg`](https://github.com/Gromsi/minSVG) binary. Not wasm / napi.

**The Rust binary is not in this tarball** (keeps `npm pack` to a few KB — no `target/`, crate sources, or fixtures). Install it separately, then put it on `PATH` or set `MINSVG_BIN`:

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
```

```js
import { optimize } from 'minsvg'

const { data } = optimize(svgString)
```

The `minsvg` bin shim in this package finds that binary and will not recurse into itself.
