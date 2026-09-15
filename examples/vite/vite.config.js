/**
 * Copy into your Vite app. You own this plugin — it is not vite-plugin-minsvg.
 * `optimize()` spawnSyncs the Rust `minsvg` binary (PATH or MINSVG_BIN).
 * `vite build` only. Not webpack. Prefer `minsvg -f src/assets --recursive` in prebuild.
 */
import { defineConfig } from 'vite'
import { optimize } from 'minsvg'

function minsvgAssets(options = {}) {
  return {
    name: 'minsvg-assets',
    generateBundle(_opts, bundle) {
      for (const item of Object.values(bundle)) {
        if (item.type !== 'asset' || !String(item.fileName).endsWith('.svg')) continue
        const source =
          typeof item.source === 'string'
            ? item.source
            : Buffer.from(item.source).toString('utf8')
        const result = optimize(source, options)
        if (result.error) throw result.modernError ?? new Error(result.error)
        item.source = result.data
      }
    },
  }
}

export default defineConfig({
  plugins: [minsvgAssets()],
})
