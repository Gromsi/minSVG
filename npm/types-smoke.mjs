import { chmodSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { optimize } from './index.js'

const here = dirname(fileURLToPath(import.meta.url))

function assert(cond, msg, extra) {
  if (!cond) {
    console.error(msg, extra ?? '')
    process.exit(1)
  }
}

function assertData(result, msg) {
  assert(result != null && typeof result === 'object', `${msg}: result is not an object`, result)
  assert(Object.hasOwn(result, 'data'), `${msg}: optimize() must return { data } like SVGO`, result)
  assert(typeof result.data === 'string', `${msg}: data must be a string`, result)
}

const dts = readFileSync(join(here, 'index.d.ts'), 'utf8')
assert(/export interface OptimizeResult\b/.test(dts), 'index.d.ts must export OptimizeResult')
assert(/^\s*data:\s*string\b/m.test(dts), 'OptimizeResult.data must be a required string')
assert(!/^\s*data\?:\s*string\b/m.test(dts), 'OptimizeResult.data must not be optional')
assert(
  /export function optimize\(\s*svg:\s*string,\s*options\?:\s*OptimizeOptions\s*\):\s*OptimizeResult\b/.test(
    dts,
  ),
  'optimize() must be typed to return OptimizeResult { data }',
)

const pkg = JSON.parse(readFileSync(join(here, 'package.json'), 'utf8'))
assert(pkg.types === './index.d.ts', 'package.json types must point at index.d.ts', pkg.types)
assert(
  pkg.exports?.['.']?.types === './index.d.ts',
  'exports["."].types must point at index.d.ts',
  pkg.exports,
)

const input =
  '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="#000000"/></svg>'
const fakeBin = join(tmpdir(), `minsvg-types-smoke-${process.pid}.mjs`)
writeFileSync(
  fakeBin,
  `#!/usr/bin/env node
process.stdout.write('<svg xmlns="http://www.w3.org/2000/svg"/>')
`,
)
chmodSync(fakeBin, 0o755)

const result = optimize(input, { bin: fakeBin })
assertData(result, 'types-smoke optimize()')
const { data } = result
assert(data.includes('<svg'), 'destructuring { data } must yield SVG:', data)
assert(!Object.hasOwn(result, 'error'), 'success should omit error:', result)

const missing = optimize(input, { bin: join(tmpdir(), `minsvg-types-missing-${process.pid}`) })
assertData(missing, 'types-smoke ENOENT optimize()')

console.log('ok types-smoke', data.length)
