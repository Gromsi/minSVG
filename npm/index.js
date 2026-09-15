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
 * @param {string} svg
 * @param {{ bin?: string }} [options]
 * @returns {{ data: string }}
 */
export function optimize(svg, options = {}) {
  if (typeof svg !== 'string') {
    throw new TypeError('optimize(svg) expects a string')
  }
  const bin = resolveMinsvgBin(options.bin)
  const result = spawnSync(bin, ['--stdin'], {
    input: svg,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (result.error) {
    if (result.error.code === 'ENOENT') {
      throw new Error(
        `minsvg binary not found (${bin}). Install with:\n  cargo install --git https://github.com/Gromsi/minSVG --locked\nOr set MINSVG_BIN to the binary path.`,
      )
    }
    throw result.error
  }
  if (result.status !== 0) {
    throw new Error((result.stderr || result.stdout || 'minsvg failed').trim())
  }
  return { data: result.stdout }
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
