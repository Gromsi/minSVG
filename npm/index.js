import { spawnSync } from 'node:child_process'
import { closeSync, openSync, readSync, realpathSync, statSync } from 'node:fs'
import { delimiter, join } from 'node:path'

const WIN = process.platform === 'win32'

/**
 * Resolve the Rust `minsvg` binary (PATH or `MINSVG_BIN`).
 * Skips this package's Node shim so `bin` / npm scripts do not recurse.
 */
export function resolveMinsvgBin(explicit) {
  if (explicit) return explicit
  if (process.env.MINSVG_BIN) return process.env.MINSVG_BIN

  const self = realpathOrNull(process.argv[1])
  const names = WIN ? ['minsvg.exe', 'minsvg.cmd', 'minsvg'] : ['minsvg']
  for (const dir of (process.env.PATH || '').split(delimiter)) {
    if (!dir) continue
    for (const name of names) {
      const candidate = join(dir, name)
      if (!isFile(candidate)) continue
      const real = realpathOrNull(candidate)
      if (self && real && self === real) continue
      if (isNodeShim(candidate)) continue
      return candidate
    }
  }
  return WIN ? 'minsvg.exe' : 'minsvg'
}

/**
 * SVGO-shaped `plugins` → `--skip` / `--plugin` / `--param`.
 * `{ name, active: false }` and `preset-default` overrides map to the skip list.
 * `{ name, params }` maps to `--plugin name` plus `--param name=<json>`.
 * Last entry for a name wins. Does not replace the default pipeline.
 *
 * @param {Array<string|{name?: string, active?: boolean, params?: Record<string, unknown>}>|null|undefined} plugins
 * @returns {{ skip: string[], plugin: string[], param: { name: string, json: string }[] }}
 */
export function mapPlugins(plugins) {
  const skip = []
  const plugin = []
  const param = []
  if (plugins == null) return { skip, plugin, param }
  if (!Array.isArray(plugins)) {
    throw new TypeError('optimize options.plugins must be an array')
  }

  for (const item of plugins) {
    if (item == null) continue
    if (item === 'preset-default') continue
    if (typeof item === 'string') {
      setMapped(skip, plugin, param, item, true)
      continue
    }
    if (typeof item !== 'object') continue
    if (item.name === 'preset-default') {
      mapPresetOverrides(skip, plugin, param, item.params?.overrides)
      continue
    }
    if (typeof item.name !== 'string' || !item.name) continue
    const enabled = item.active !== false
    setMapped(skip, plugin, param, item.name, enabled, enabled ? item.params : undefined)
  }
  return { skip, plugin, param }
}

/**
 * SVGO-shaped knobs → `minsvg` argv (`--stdin` plus optional flags).
 * CLI has no `--path`; `options.path` is accepted by `optimize()` only (error file).
 * @param {{ multipass?: boolean, precision?: number, plugins?: unknown[] }} [options]
 * @returns {string[]}
 */
export function optimizeArgs(options = {}) {
  const args = ['--stdin']
  if (options.multipass) args.push('--multipass')
  if (options.precision != null) args.push('--precision', String(options.precision))
  const { skip, plugin, param } = mapPlugins(options.plugins)
  for (const name of skip) args.push('--skip', name)
  for (const name of plugin) args.push('--plugin', name)
  for (const spec of param) args.push('--param', `${spec.name}=${spec.json}`)
  return args
}

/**
 * @param {string} svg
 * @param {{ bin?: string, multipass?: boolean, precision?: number, path?: string, plugins?: unknown[] }} [options]
 * @returns {{ data: string, error?: string, modernError?: Error }}
 */
export function optimize(svg, options = {}) {
  if (typeof svg !== 'string') {
    throw new TypeError('optimize(svg) expects a string')
  }
  const bin = resolveMinsvgBin(options.bin)
  let result
  try {
    result = spawnSync(bin, optimizeArgs(options), {
      input: svg,
      encoding: 'utf8',
      maxBuffer: 32 * 1024 * 1024,
    })
  } catch (err) {
    return failResult(err, options, bin)
  }
  if (result.error) {
    return failResult(result.error, options, bin)
  }
  if (result.status !== 0) {
    const message = (result.stderr || result.stdout || 'minsvg failed').trim()
    return failResult(new Error(message), options, bin, result.stdout)
  }
  return { data: result.stdout ?? '' }
}

function mapPresetOverrides(skip, plugin, param, overrides) {
  if (!overrides || typeof overrides !== 'object') return
  for (const [name, val] of Object.entries(overrides)) {
    if (isDisabledOverride(val)) {
      setMapped(skip, plugin, param, name, false)
      continue
    }
    if (val === true) {
      setMapped(skip, plugin, param, name, true)
      continue
    }
    if (!isPlainObject(val)) continue
    const enabled = val.active !== false
    setMapped(skip, plugin, param, name, enabled, enabled ? omitActive(val) : undefined)
  }
}

function failResult(err, options, bin, stdout) {
  const modernError = wrapSpawnError(err, bin, options?.path)
  return {
    data: typeof stdout === 'string' ? stdout : '',
    error: modernError.message,
    modernError,
  }
}

function wrapSpawnError(err, bin, path) {
  const code = err && typeof err === 'object' ? err.code : undefined
  const message =
    code === 'ENOENT'
      ? `minsvg binary not found (${bin ?? 'minsvg'}). Install with:\n  cargo install --git https://github.com/Gromsi/minSVG --locked\nOr set MINSVG_BIN to the binary path.`
      : err instanceof Error
        ? err.message
        : String(err)
  const modernError = new Error(message)
  modernError.name = 'SvgoParserError'
  modernError.reason = message
  if (code) modernError.code = code
  if (path) modernError.file = path
  if (err instanceof Error) modernError.cause = err
  return modernError
}

function omitActive(val) {
  const rest = { ...val }
  delete rest.active
  return rest
}

function isDisabledOverride(val) {
  return val === false || (isPlainObject(val) && val.active === false)
}

function isPlainObject(val) {
  return val != null && typeof val === 'object' && !Array.isArray(val)
}

function cliParams(params) {
  if (!isPlainObject(params)) return undefined
  return Object.keys(params).length ? params : undefined
}

function setMapped(skip, plugin, param, name, enabled, params) {
  if (typeof name !== 'string' || !name) return
  pullIgnoreCase(enabled ? skip : plugin, name)
  pullParam(param, name)
  const dest = enabled ? plugin : skip
  if (!dest.some((s) => s.toLowerCase() === name.toLowerCase())) dest.push(name)
  const jsonVal = enabled ? cliParams(params) : undefined
  if (jsonVal) param.push({ name, json: JSON.stringify(jsonVal) })
}

function pullParam(list, name) {
  const i = list.findIndex((p) => p.name.toLowerCase() === name.toLowerCase())
  if (i >= 0) list.splice(i, 1)
}

function pullIgnoreCase(list, name) {
  const i = list.findIndex((s) => s.toLowerCase() === name.toLowerCase())
  if (i >= 0) list.splice(i, 1)
}

function isFile(path) {
  try {
    return statSync(path).isFile()
  } catch {
    return false
  }
}

function realpathOrNull(path) {
  if (!path) return null
  try {
    return realpathSync(path)
  } catch {
    return null
  }
}

function isNodeShim(path) {
  let fd
  try {
    fd = openSync(path, 'r')
    const buf = Buffer.alloc(80)
    const n = readSync(fd, buf, 0, 80, 0)
    const head = buf.toString('utf8', 0, n)
    return head.startsWith('#!') && /\bnode\b/.test(head)
  } catch {
    return false
  } finally {
    if (fd !== undefined) closeSync(fd)
  }
}
