import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { optimize } from './index.js'

const here = dirname(fileURLToPath(import.meta.url))
if (!process.env.MINSVG_BIN) {
  for (const rel of ['../target/debug/minsvg', '../target/release/minsvg']) {
    const candidate = resolve(here, rel)
    if (existsSync(candidate)) {
      process.env.MINSVG_BIN = candidate
      break
    }
  }
}

const input =
  '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="#000000"/></svg>'
const { data } = optimize(input)
if (typeof data !== 'string' || !data.includes('<svg')) {
  console.error('optimize() did not return SVG:', data)
  process.exit(1)
}
console.log('ok', data.length)
