/**
 * Attach this handler to *your* API Gateway or Function URL.
 * Put the `minsvg` binary on PATH (container image, layer, or zip).
 * We do not host this and do not open a public URL for you.
 */
import { spawnSync } from 'node:child_process'

const BIN = process.env.MINSVG_BIN || 'minsvg'

export async function handler(event) {
  const raw = event?.body ?? ''
  const body = event?.isBase64Encoded
    ? Buffer.from(raw, 'base64').toString('utf8')
    : String(raw)

  const contentType = header(event?.headers, 'content-type')
  const asJson =
    contentType.includes('application/json') || body.trimStart().startsWith('{')

  let svg = body
  if (asJson) {
    let parsed
    try {
      parsed = JSON.parse(body)
    } catch (err) {
      return json(400, { error: `json: ${err.message}` })
    }
    if (typeof parsed?.svg !== 'string') {
      return json(400, { error: 'missing svg field' })
    }
    svg = parsed.svg
  }

  const result = spawnSync(BIN, ['--stdin'], {
    input: svg,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  })
  if (result.error?.code === 'ENOENT') {
    return json(500, {
      error: 'minsvg binary not found. Add it to the image/layer or set MINSVG_BIN.',
    })
  }
  if (result.status !== 0) {
    return json(400, {
      error: (result.stderr || result.stdout || 'minsvg failed').trim(),
    })
  }
  if (asJson) {
    return json(200, { data: result.stdout })
  }
  return {
    statusCode: 200,
    headers: { 'content-type': 'image/svg+xml; charset=utf-8' },
    body: result.stdout,
  }
}

function header(headers, name) {
  for (const [key, value] of Object.entries(headers || {})) {
    if (String(key).toLowerCase() === name) return String(value).toLowerCase()
  }
  return ''
}

function json(statusCode, obj) {
  return {
    statusCode,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(obj),
  }
}
