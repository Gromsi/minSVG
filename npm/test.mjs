import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mapPlugins, optimize, optimizeArgs } from './index.js'
import './types-smoke.mjs'

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

function paramOf(mapped, name) {
  return mapped.param.find((p) => p.name.toLowerCase() === name.toLowerCase())
}

function parseParamArgv(argv, name) {
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] !== '--param') continue
    const spec = argv[i + 1]
    const eq = spec.indexOf('=')
    if (eq < 0) continue
    if (spec.slice(0, eq) === name) return JSON.parse(spec.slice(eq + 1))
  }
  return undefined
}

const emptyMap = { skip: [], plugin: [], param: [] }
assert(JSON.stringify(mapPlugins(undefined)) === JSON.stringify(emptyMap), 'nullish plugins')
assert(JSON.stringify(mapPlugins(null)) === JSON.stringify(emptyMap), 'null plugins')
assert(JSON.stringify(mapPlugins([])) === JSON.stringify(emptyMap), 'empty plugins')

const input =
  '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="#000000"/></svg>'
const result = optimize(input)
assertData(result, 'default optimize')
assert(result.data.includes('<svg'), 'optimize() did not return SVG:', result.data)
assert(!Object.hasOwn(result, 'error'), 'success should omit error:', result)
assert(!Object.hasOwn(result, 'modernError'), 'success should omit modernError:', result)

const flagged = optimizeArgs({ multipass: true, precision: 3 })
assert(
  flagged[0] === '--stdin' &&
    flagged.includes('--multipass') &&
    flagged[flagged.indexOf('--precision') + 1] === '3',
  'optimizeArgs did not pass { multipass, precision }:',
  flagged,
)
const plain = optimizeArgs({})
assert(
  !plain.includes('--multipass') && !plain.includes('--precision') && !plain.includes('--path'),
  'optimizeArgs should omit unset flags:',
  plain,
)
assert(
  JSON.stringify(optimizeArgs({ path: 'icon.svg' })) === JSON.stringify(['--stdin']),
  'CLI has no --path; optimizeArgs must ignore path:',
  optimizeArgs({ path: 'icon.svg' }),
)

const disabled = mapPlugins([{ name: 'removeComments', active: false }])
assert(
  JSON.stringify(disabled) ===
    JSON.stringify({ skip: ['removeComments'], plugin: [], param: [] }),
  'active:false must map to skip list:',
  disabled,
)

const enabled = mapPlugins([{ name: 'cleanupIds', active: true }])
assert(
  JSON.stringify(enabled) === JSON.stringify({ skip: [], plugin: ['cleanupIds'], param: [] }),
  'active:true must map to --plugin:',
  enabled,
)

const withParams = mapPlugins([
  { name: 'cleanupListOfValues', params: { floatPrecision: 2, leadingZero: false } },
])
assert(
  JSON.stringify(withParams.skip) === JSON.stringify([]) &&
    JSON.stringify(withParams.plugin) === JSON.stringify(['cleanupListOfValues']),
  '{ name, params } must enable --plugin:',
  withParams,
)
assert(
  JSON.stringify(JSON.parse(paramOf(withParams, 'cleanupListOfValues').json)) ===
    JSON.stringify({ floatPrecision: 2, leadingZero: false }),
  '{ name, params } must map to --param JSON:',
  withParams,
)

const skippedWithParams = mapPlugins([
  { name: 'cleanupListOfValues', active: false, params: { floatPrecision: 2 } },
])
assert(
  JSON.stringify(skippedWithParams) ===
    JSON.stringify({ skip: ['cleanupListOfValues'], plugin: [], param: [] }),
  'active:false must skip and drop params:',
  skippedWithParams,
)

const preset = mapPlugins([
  {
    name: 'preset-default',
    params: {
      overrides: {
        convertPathData: false,
        mergePaths: { active: false },
        cleanupIds: true,
        cleanupListOfValues: { floatPrecision: 2 },
      },
    },
  },
])
assert(
  JSON.stringify(preset.skip) === JSON.stringify(['convertPathData', 'mergePaths']) &&
    JSON.stringify(preset.plugin) === JSON.stringify(['cleanupIds', 'cleanupListOfValues']),
  'preset-default overrides must map to skip/plugin:',
  preset,
)
assert(
  JSON.parse(paramOf(preset, 'cleanupListOfValues').json).floatPrecision === 2,
  'preset-default object override must become --param:',
  preset,
)

const lastWins = mapPlugins([
  { name: 'removeComments', active: true },
  { name: 'removeComments', active: false },
])
assert(
  JSON.stringify(lastWins) ===
    JSON.stringify({ skip: ['removeComments'], plugin: [], param: [] }),
  'last plugin entry should win:',
  lastWins,
)

const lastParamWins = mapPlugins([
  { name: 'cleanupListOfValues', params: { floatPrecision: 1 } },
  { name: 'cleanupListOfValues', params: { floatPrecision: 2 } },
])
assert(
  JSON.parse(paramOf(lastParamWins, 'cleanupListOfValues').json).floatPrecision === 2 &&
    lastParamWins.param.length === 1,
  'last params entry should win:',
  lastParamWins,
)

const unparam = mapPlugins([
  { name: 'cleanupListOfValues', params: { floatPrecision: 2 } },
  { name: 'cleanupListOfValues', active: false },
])
assert(
  JSON.stringify(unparam) ===
    JSON.stringify({ skip: ['cleanupListOfValues'], plugin: [], param: [] }),
  'later active:false should drop params:',
  unparam,
)

const pluginArgv = optimizeArgs({
  plugins: [
    { name: 'preset-default', params: { overrides: { removeComments: false } } },
    { name: 'cleanupIds', active: true },
    { name: 'cleanupListOfValues', params: { floatPrecision: 2 } },
  ],
})
assert(pluginArgv[0] === '--stdin', 'optimizeArgs must start with --stdin:', pluginArgv)
assert(
  pluginArgv.includes('--skip') && pluginArgv[pluginArgv.indexOf('--skip') + 1] === 'removeComments',
  'optimizeArgs plugins mapping skip mismatch:',
  pluginArgv,
)
assert(pluginArgv.includes('--plugin'), 'optimizeArgs must pass --plugin:', pluginArgv)
assert(
  pluginArgv.includes('cleanupIds') && pluginArgv.includes('cleanupListOfValues'),
  'optimizeArgs must pass plugin names:',
  pluginArgv,
)
assert(parseParamArgv(pluginArgv, 'cleanupListOfValues').floatPrecision === 2, 'optimizeArgs --param', pluginArgv)
assert(!pluginArgv.includes('preset-default'), 'preset-default is not a CLI plugin:', pluginArgv)

let threw = false
try {
  mapPlugins({ name: 'removeComments', active: false })
} catch (err) {
  threw = err instanceof TypeError
}
assert(threw, 'mapPlugins must reject a non-array plugins value')

threw = false
try {
  optimize({ not: 'a string' })
} catch (err) {
  threw = err instanceof TypeError
}
assert(threw, 'optimize() must reject a non-string svg')

const argvFile = join(tmpdir(), `minsvg-argv-${process.pid}.json`)
const fakeBin = join(tmpdir(), `minsvg-fake-${process.pid}.mjs`)
writeFileSync(
  fakeBin,
  `#!/usr/bin/env node
import { writeFileSync } from 'node:fs'
writeFileSync(${JSON.stringify(argvFile)}, JSON.stringify(process.argv.slice(2)))
process.stdout.write('<svg xmlns="http://www.w3.org/2000/svg"/>')
`,
)
chmodSync(fakeBin, 0o755)
const spawned = optimize(input, { bin: fakeBin, multipass: true, precision: 2 })
assertData(spawned, 'optimize({ multipass, precision })')
assert(spawned.data.includes('<svg'), 'fake-bin optimize did not return SVG:', spawned)
const argv = JSON.parse(readFileSync(argvFile, 'utf8'))
assert(
  JSON.stringify(argv) === JSON.stringify(['--stdin', '--multipass', '--precision', '2']),
  'binary argv mismatch:',
  argv,
)

const pluginSpawned = optimize(input, {
  bin: fakeBin,
  plugins: [{ name: 'removeComments', active: false }],
})
assertData(pluginSpawned, 'plugin optimize()')
const pluginArgvSpawned = JSON.parse(readFileSync(argvFile, 'utf8'))
assert(
  JSON.stringify(pluginArgvSpawned) === JSON.stringify(['--stdin', '--skip', 'removeComments']),
  'active:false must spawn --skip:',
  pluginArgvSpawned,
)

const paramSpawned = optimize(input, {
  bin: fakeBin,
  plugins: [{ name: 'cleanupListOfValues', params: { floatPrecision: 2 } }],
})
assertData(paramSpawned, 'params optimize()')
const paramArgvSpawned = JSON.parse(readFileSync(argvFile, 'utf8'))
assert(paramArgvSpawned.includes('--plugin'), 'params must spawn --plugin:', paramArgvSpawned)
assert(
  paramArgvSpawned[paramArgvSpawned.indexOf('--plugin') + 1] === 'cleanupListOfValues',
  'params must spawn --plugin name:',
  paramArgvSpawned,
)
assert(
  parseParamArgv(paramArgvSpawned, 'cleanupListOfValues').floatPrecision === 2,
  'params must spawn --param name=json:',
  paramArgvSpawned,
)

const missingBin = join(tmpdir(), `minsvg-missing-${process.pid}`)
let missingThrew = false
let missing
try {
  missing = optimize(input, { bin: missingBin, path: 'icon.svg' })
} catch {
  missingThrew = true
}
assert(!missingThrew, 'spawn failure must not throw raw')
assertData(missing, 'ENOENT optimize()')
assert(typeof missing.error === 'string' && missing.error.length > 0, 'ENOENT must set error:', missing)
assert(missing.modernError instanceof Error, 'ENOENT must set modernError:', missing)
assert(
  missing.modernError.code === 'ENOENT' || /not found/i.test(missing.error),
  'ENOENT error should mention the missing binary:',
  missing,
)
assert(missing.modernError.file === 'icon.svg', 'path should land on modernError.file:', missing.modernError)

const failBin = join(tmpdir(), `minsvg-fail-${process.pid}.mjs`)
writeFileSync(
  failBin,
  `#!/usr/bin/env node
process.stderr.write('minsvg: boom')
process.exit(2)
`,
)
chmodSync(failBin, 0o755)
let failThrew = false
let failed
try {
  failed = optimize(input, { bin: failBin, path: 'broken.svg' })
} catch {
  failThrew = true
}
assert(!failThrew, 'non-zero exit must not throw raw')
assertData(failed, 'non-zero optimize()')
assert(typeof failed.error === 'string' && failed.error.includes('boom'), 'non-zero must set error:', failed)
assert(failed.modernError instanceof Error, 'non-zero must set modernError:', failed)
assert(failed.modernError.file === 'broken.svg', 'path should land on modernError.file:', failed.modernError)
assert(failed.modernError.name === 'SvgoParserError', 'modernError name should be SVGO-shaped:', failed.modernError)

const commented =
  '<svg xmlns="http://www.w3.org/2000/svg"><!-- keep --><rect width="10" height="10"/></svg>'
const stripped = optimize(commented)
assertData(stripped, 'default comment optimize')
assert(!stripped.data.includes('keep'), 'default optimize should strip comments:', stripped.data)
assert(!Object.hasOwn(stripped, 'error'), 'successful optimize must not set error')

const kept = optimize(commented, { plugins: [{ name: 'removeComments', active: false }] })
assertData(kept, 'skipped-comment optimize')
assert(kept.data.includes('keep'), 'active:false removeComments should keep the comment:', kept.data)

const unskipped = optimize(commented, {
  plugins: [
    { name: 'removeComments', active: false },
    { name: 'removeComments', active: true },
  ],
})
assertData(unskipped, 'unskipped-comment optimize')
assert(
  !unskipped.data.includes('keep'),
  'later active:true should unskip removeComments:',
  unskipped.data,
)

const lists =
  '<svg xmlns="http://www.w3.org/2000/svg"><polygon points="0pt,12pt 1.239,2"/></svg>'
const listsDefault = optimize(lists)
assertData(listsDefault, 'lists default')
const listsParam = optimize(lists, {
  plugins: [{ name: 'cleanupListOfValues', params: { floatPrecision: 2 } }],
})
assertData(listsParam, 'lists params')
assert(!Object.hasOwn(listsParam, 'error'), 'params optimize should succeed:', listsParam)
assert(
  listsParam.data.includes('16') && !listsParam.data.includes('12pt'),
  '{ name, params } should enable cleanupListOfValues via --plugin/--param:',
  listsParam.data,
)

console.log('ok', result.data.length)
