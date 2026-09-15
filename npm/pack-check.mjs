import { spawnSync } from 'node:child_process'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))

// Budget without the Rust binary. Today's wrapper is ~10 KB unpacked;
// a stray `target/` binary (~1.4 MB) or fixtures would fail this.
const MAX_UNPACKED = 256 * 1024
const MAX_PACKED = 128 * 1024

const blocked = [
  /(^|\/)target\//,
  /(^|\/)src\//,
  /(^|\/)tests\//,
  /(^|\/)fixtures\//,
  /(^|\/)corpus\//,
  /\.rs$/i,
  /(^|\/)Cargo\.(toml|lock)$/i,
  /(^|\/)minsvg(\.exe)?$/,
  /\.(crate|node|wasm)$/i,
]

function fail(msg, extra) {
  console.error(msg, extra ?? '')
  process.exit(1)
}

const packed = spawnSync('npm', ['pack', '--dry-run', '--json'], {
  cwd: here,
  encoding: 'utf8',
})
if (packed.status !== 0) {
  fail('npm pack --dry-run failed:', packed.stderr || packed.stdout)
}

const report = parsePackJson(packed.stdout)
if (!report) fail('npm pack --json: expected a package report', packed.stdout)

const files = report.files ?? []
const paths = files.map((f) => f.path)
for (const path of paths) {
  if (blocked.some((re) => re.test(path))) {
    fail('tarball must not include Rust/build/fixture path:', path)
  }
}

if (report.unpackedSize > MAX_UNPACKED) {
  fail(
    `unpacked size ${report.unpackedSize} exceeds ${MAX_UNPACKED} (Rust binary is optional and must not be packed)`,
  )
}
if (report.size > MAX_PACKED) {
  fail(`packed size ${report.size} exceeds ${MAX_PACKED}`)
}

console.log(
  `ok pack ${report.filename} packed=${report.size} unpacked=${report.unpackedSize} files=${report.entryCount}`,
)
console.log(paths.join('\n'))

function parsePackJson(stdout) {
  const start = stdout.indexOf('[')
  const end = stdout.lastIndexOf(']')
  if (start < 0 || end < 0) return null
  const arr = JSON.parse(stdout.slice(start, end + 1))
  return arr[0] ?? null
}
