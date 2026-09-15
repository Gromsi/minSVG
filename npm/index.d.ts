export type PluginOverride = boolean | { active?: boolean; [key: string]: unknown }

export type PluginConfig =
  | string
  | {
      name: string
      active?: boolean
      params?: {
        overrides?: Record<string, PluginOverride>
        [key: string]: unknown
      }
    }

export interface PluginParamSpec {
  name: string
  json: string
}

export interface MappedPlugins {
  skip: string[]
  plugin: string[]
  param: PluginParamSpec[]
}

export interface OptimizeOptions {
  /** Absolute path to the Rust `minsvg` binary. Overrides PATH and `MINSVG_BIN`. */
  bin?: string
  /** Re-run passes until output is stable. Maps to `--multipass`. */
  multipass?: boolean
  /** Fractional digits. Maps to `--precision`. */
  precision?: number
  /**
   * SVGO `path` (source file). CLI has no `--path`; used as `modernError.file`
   * on spawn / process failure.
   */
  path?: string
  /**
   * SVGO-shaped plugin list.
   * `{ name, active: false }` / `preset-default` overrides → `--skip`.
   * `{ name, active: true }` or a string name → `--plugin` (unskip / opt-in).
   * `{ name, params }` → `--plugin name` plus `--param name=<json>`.
   */
  plugins?: PluginConfig[]
}

export interface OptimizeResult {
  /** Optimized SVG (SVGO-shaped field name). Always present. */
  data: string
  /** Set on spawn / process failure (SVGO-shaped). */
  error?: string
  /** Error object on spawn / process failure (SVGO-shaped). */
  modernError?: Error
}

export function resolveMinsvgBin(explicit?: string): string
export function mapPlugins(plugins?: PluginConfig[] | null): MappedPlugins
export function optimizeArgs(
  options?: Pick<OptimizeOptions, 'multipass' | 'precision' | 'plugins'>,
): string[]
export function optimize(svg: string, options?: OptimizeOptions): OptimizeResult
