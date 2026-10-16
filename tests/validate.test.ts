import { describe, expect, it } from 'vitest'
import {
  array,
  number,
  object,
  oneOf,
  strictPartial,
  string,
  withDefault
} from '../src/main/validate'

describe('validate', () => {
  it('drops unknown fields from objects', () => {
    const s = object<{ a: string }>({ a: string() })
    expect(s.parse({ a: 'x', extra: 1, __proto__: { evil: true } })).toEqual({ a: 'x' })
  })

  it('rejects unknown fields in strict partials and skips missing ones', () => {
    const s = strictPartial<{ a: string; b: number }>({ a: string(), b: number() })
    expect(s.parse({ b: 2 })).toEqual({ b: 2 })
    expect(() => s.parse({ c: 1 })).toThrow()
    expect(() => s.parse(JSON.parse('{"__proto__": {}}'))).toThrow()
  })

  it('rejects non-finite numbers and out-of-range values', () => {
    expect(() => number().parse(NaN)).toThrow()
    expect(() => number().parse(Infinity)).toThrow()
    expect(() => number({ max: 1 }).parse(1.5)).toThrow()
    expect(() => string({ max: 2 }).parse('abc')).toThrow()
    expect(() => oneOf(['a', 'b']).parse('c')).toThrow()
    expect(() => array(string(), { max: 1 }).parse(['a', 'b'])).toThrow()
  })

  it('gives each default its own copy', () => {
    const s = withDefault(array(string()), [] as string[])
    const first = s.parse(undefined)
    first.push('x')
    expect(s.parse(undefined)).toEqual([])
  })

  it('safeParse reports failure instead of throwing', () => {
    expect(number().safeParse('1')).toEqual({ success: false })
    expect(number().safeParse(1)).toEqual({ success: true, data: 1 })
  })
})
