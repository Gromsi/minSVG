# minsvg

SVGO-shaped `optimize()` that **spawns** the Rust [`minsvg`](https://github.com/Gromsi/minSVG) binary. Not wasm / napi.

**The Rust binary is not in this tarball** (packed ~4 KB — no `target/`, crate sources, or fixtures). The default Rust CLI is barely **~2 MB** (LTO/strip; `mcp`/`serve` feature-gated). Exact bytes: repo [`PACKAGE_SIZE.md`](https://github.com/Gromsi/minSVG/blob/main/PACKAGE_SIZE.md). Install the CLI separately, then put it on `PATH` or set `MINSVG_BIN`:

```bash
cargo install --git https://github.com/Gromsi/minSVG --locked
```

```js
import { optimize } from 'minsvg'

const { data } = optimize(svgString)
```

The `minsvg` bin shim in this package finds that binary and will not recurse into itself.
