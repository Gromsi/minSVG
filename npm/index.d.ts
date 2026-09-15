export interface OptimizeOptions {
  /** Absolute path to the Rust `minsvg` binary. Overrides PATH and `MINSVG_BIN`. */
  bin?: string
}

export interface OptimizeResult {
  /** Optimized SVG (SVGO-shaped field name). */
  data: string
}

export function resolveMinsvgBin(explicit?: string): string
export function optimize(svg: string, options?: OptimizeOptions): OptimizeResult
