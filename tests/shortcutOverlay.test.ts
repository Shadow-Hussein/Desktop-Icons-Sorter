import { describe, expect, it, vi } from 'vitest'

// shortcutOverlay imports electron for nativeImage; only the parser is tested here.
vi.mock('electron', () => ({ nativeImage: {} }))
const { DEFAULT_ARROW, parseShellIconValue } = await import('../src/main/shortcutOverlay')

const header =
  '\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Icons\r\n'

describe('parseShellIconValue', () => {
  it('reads a custom arrow with an index', () => {
    const out = `${header}    29    REG_SZ    C:\\Icons\\purple-arrow.ico,0\r\n\r\n`
    expect(parseShellIconValue(out)).toEqual({ file: 'C:\\Icons\\purple-arrow.ico', index: 0 })
  })

  it('handles REG_EXPAND_SZ, negative resource ids and commas in paths', () => {
    const out = `${header}    29    REG_EXPAND_SZ    %SystemRoot%\\a,b\\imageres.dll,-163\r\n`
    expect(parseShellIconValue(out)).toEqual({
      file: '%SystemRoot%\\a,b\\imageres.dll',
      index: -163
    })
  })

  it('treats a value without an index as index 0 and strips quotes', () => {
    const out = `${header}    29    REG_SZ    "C:\\Program Files\\Arrows\\blank.ico"\r\n`
    expect(parseShellIconValue(out)).toEqual({
      file: 'C:\\Program Files\\Arrows\\blank.ico',
      index: 0
    })
  })

  it('returns null when the value is absent', () => {
    expect(parseShellIconValue(`${header}    3    REG_SZ    C:\\x.ico,0\r\n`)).toBeNull()
    expect(parseShellIconValue('')).toBeNull()
  })

  it('defaults to the built-in imageres arrow', () => {
    expect(DEFAULT_ARROW).toEqual({ file: '%SystemRoot%\\System32\\imageres.dll', index: -163 })
  })
})
