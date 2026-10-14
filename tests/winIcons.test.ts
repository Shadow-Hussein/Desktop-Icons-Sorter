import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { extractIcon } from '../src/main/winIcons'

const isWindows = process.platform === 'win32'
const shell32 = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'shell32.dll')

describe('extractIcon', () => {
  it.runIf(!isWindows)('is a no-op off Windows', () => {
    expect(extractIcon('/bin/sh', 0, 48)).toBeNull()
  })

  // These run on the windows-latest CI job against real system icons.
  it.runIf(isWindows).each([16, 48, 64, 128])('extracts a %ipx icon from shell32.dll', (size) => {
    const icon = extractIcon(shell32, 3, size) // index 3: closed folder
    expect(icon).not.toBeNull()
    expect(icon!.width).toBe(size)
    expect(icon!.height).toBe(size)
    expect(icon!.data.length).toBe(size * size * 4)
    let opaque = 0
    let transparent = 0
    for (let i = 3; i < icon!.data.length; i += 4) {
      if (icon!.data[i] === 255) opaque++
      if (icon!.data[i] === 0) transparent++
    }
    // A real icon has both a visible shape and transparent surroundings.
    expect(opaque).toBeGreaterThan(size)
    expect(transparent).toBeGreaterThan(0)
  })

  it.runIf(isWindows)('keeps premultiplied colour channels within alpha', () => {
    const icon = extractIcon(shell32, 3, 48)!
    for (let i = 0; i < icon.data.length; i += 4) {
      const a = icon.data[i + 3]
      expect(Math.max(icon.data[i], icon.data[i + 1], icon.data[i + 2])).toBeLessThanOrEqual(a)
    }
  })

  it.runIf(isWindows)('returns null for missing files and icons', () => {
    expect(extractIcon('C:\\does\\not\\exist.exe', 0, 48)).toBeNull()
    expect(extractIcon(shell32, 100000, 48)).toBeNull()
    expect(extractIcon(join(process.env.SystemRoot ?? 'C:\\Windows', 'win.ini'), 0, 48)).toBeNull()
  })
})
