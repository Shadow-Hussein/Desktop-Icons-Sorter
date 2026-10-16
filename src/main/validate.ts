/**
 * Minimal runtime validation for config files and IPC payloads.
 *
 * Stands in for zod, which cost ~13 MB of main-process memory just to load
 * for the handful of checks this app needs. Objects keep only the fields
 * their shape names, like zod's default (strip) mode.
 */

export class ValidationError extends Error {}

export type SafeParseResult<T> = { success: true; data: T } | { success: false }

export interface Schema<T> {
  /** Returns the validated value, or throws a ValidationError. */
  parse(value: unknown, path?: string): T
  safeParse(value: unknown): SafeParseResult<T>
}

function fail(path: string, message: string): never {
  throw new ValidationError(`${path || 'value'}: ${message}`)
}

function schema<T>(check: (value: unknown, path: string) => T): Schema<T> {
  return {
    parse: (value, path = '') => check(value, path),
    safeParse: (value) => {
      try {
        return { success: true, data: check(value, '') }
      } catch (err) {
        if (err instanceof ValidationError) return { success: false }
        throw err
      }
    }
  }
}

const field = (path: string, key: string): string => (path ? `${path}.${key}` : key)

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

export function string(
  opts: { min?: number; max?: number; pattern?: RegExp } = {}
): Schema<string> {
  return schema((v, path) => {
    if (typeof v !== 'string') fail(path, 'expected a string')
    if (opts.min !== undefined && v.length < opts.min) fail(path, `shorter than ${opts.min}`)
    if (opts.max !== undefined && v.length > opts.max) fail(path, `longer than ${opts.max}`)
    if (opts.pattern && !opts.pattern.test(v)) fail(path, 'invalid format')
    return v
  })
}

/** Finite numbers only (no NaN or Infinity). */
export function number(opts: { min?: number; max?: number } = {}): Schema<number> {
  return schema((v, path) => {
    if (typeof v !== 'number' || !Number.isFinite(v)) fail(path, 'expected a finite number')
    if (opts.min !== undefined && v < opts.min) fail(path, `less than ${opts.min}`)
    if (opts.max !== undefined && v > opts.max) fail(path, `greater than ${opts.max}`)
    return v
  })
}

export function boolean(): Schema<boolean> {
  return schema((v, path) => (typeof v === 'boolean' ? v : fail(path, 'expected a boolean')))
}

export function oneOf<const T extends string | number>(values: readonly T[]): Schema<T> {
  return schema((v, path) =>
    values.includes(v as T) ? (v as T) : fail(path, `expected one of ${values.join(', ')}`)
  )
}

/** Uses `fallback` when the value is missing (undefined). */
export function withDefault<T>(inner: Schema<T>, fallback: T): Schema<T> {
  return schema((v, path) => (v === undefined ? structuredClone(fallback) : inner.parse(v, path)))
}

export function array<T>(item: Schema<T>, opts: { max?: number } = {}): Schema<T[]> {
  return schema((v, path) => {
    if (!Array.isArray(v)) fail(path, 'expected an array')
    if (opts.max !== undefined && v.length > opts.max) fail(path, `more than ${opts.max} items`)
    return v.map((x, i) => item.parse(x, field(path, String(i))))
  })
}

export type Shape<T> = { [K in keyof T]-?: Schema<T[K]> }

/** An object with exactly the fields of `shape`; unknown fields are dropped. */
export function object<T>(shape: Shape<T>): Schema<T> {
  return schema((v, path) => {
    if (!isRecord(v)) fail(path, 'expected an object')
    const out = {} as T
    for (const key in shape) out[key] = shape[key].parse(v[key], field(path, key))
    return out
  })
}

/** Every field optional, and unknown fields rejected rather than dropped. */
export function strictPartial<T>(shape: Shape<T>): Schema<Partial<T>> {
  return schema((v, path) => {
    if (!isRecord(v)) fail(path, 'expected an object')
    const out: Partial<T> = {}
    for (const key of Object.keys(v)) {
      if (!Object.prototype.hasOwnProperty.call(shape, key)) fail(path, `unknown field "${key}"`)
      const k = key as keyof T & string
      if (v[k] !== undefined) out[k] = shape[k].parse(v[k], field(path, k))
    }
    return out
  })
}
