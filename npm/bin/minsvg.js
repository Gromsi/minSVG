#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { resolveMinsvgBin } from '../index.js'

const bin = resolveMinsvgBin()
const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' })
child.on('error', (err) => {
  if (err.code === 'ENOENT') {
    console.error(
      'minsvg: Rust binary not found. Install it first:\n  cargo install --git https://github.com/Gromsi/minSVG --locked',
    )
    process.exit(127)
  }
  console.error(`minsvg: ${err.message}`)
  process.exit(1)
})
child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 1)
})
