import { describe, expect, it } from 'vitest'
import {
  expandEnv,
  isIconContainer,
  parseUrlShortcut,
  resolveIconSource,
  type IconSourceDeps,
  type ShortcutDetails
} from '../src/main/iconSource'

function deps(
  over: Partial<IconSourceDeps> & { files?: string[]; links?: Record<string, ShortcutDetails> }
): IconSourceDeps {
  const files = new Set(over.files ?? [])
  return {
    readShortcutLink: (p) => {
      const link = over.links?.[p]
      if (!link) throw new Error('Failed to read shortcut link')
      return link
    },
    readFile: over.readFile ?? (async () => ''),
    exists: async (p) => files.has(p),
    env: over.env ?? {}
  }
}

describe('expandEnv', () => {
  it('expands %VAR% case-insensitively and keeps unknown ones', () => {
    const env = { SystemRoot: 'C:\\Windows', ProgramFiles: 'C:\\Program Files' }
    expect(expandEnv('%systemroot%\\System32\\shell32.dll', env)).toBe(
      'C:\\Windows\\System32\\shell32.dll'
    )
    expect(expandEnv('%PROGRAMFILES%\\App\\%NOPE%\\a.exe', env)).toBe(
      'C:\\Program Files\\App\\%NOPE%\\a.exe'
    )
  })
})

describe('parseUrlShortcut', () => {
  it('reads IconFile and IconIndex from a Steam shortcut', () => {
    const text = [
      '[{000214A0-0000-0000-C000-000000000046}]',
      'Prop3=19,0',
      '[InternetShortcut]',
      'IDList=',
      'IconIndex=0',
      'URL=steam://rungameid/2849080',
      'IconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\abc.ico'
    ].join('\r\n')
    expect(parseUrlShortcut(text)).toEqual({
      iconFile: 'C:\\Program Files (x86)\\Steam\\steam\\games\\abc.ico',
      iconIndex: 0
    })
  })

  it('handles quotes, negative indexes and keys outside the section', () => {
    const text =
      'IconFile=ignored.ico\n[InternetShortcut]\nIconFile="C:\\a b\\x.dll"\nIconIndex=-101\n'
    expect(parseUrlShortcut(text)).toEqual({ iconFile: 'C:\\a b\\x.dll', iconIndex: -101 })
  })

  it('returns no icon file for a plain web link', () => {
    expect(parseUrlShortcut('[InternetShortcut]\nURL=https://example.com\n')).toEqual({
      iconFile: undefined,
      iconIndex: 0
    })
  })
})

describe('isIconContainer', () => {
  it('recognises files with icon resources', () => {
    expect(isIconContainer('C:\\x\\Steam.EXE')).toBe(true)
    expect(isIconContainer('shell32.dll')).toBe(true)
    expect(isIconContainer('notes.txt')).toBe(false)
  })
})

describe('resolveIconSource', () => {
  const brave = 'C:\\Program Files\\BraveSoftware\\Brave-Browser\\Application\\brave.exe'

  it('uses a shortcut’s explicit icon location', async () => {
    const d = deps({
      files: [brave, 'C:\\Windows\\System32\\imageres.dll'],
      env: { SystemRoot: 'C:\\Windows' },
      links: {
        'D:\\Apps\\Brave.lnk': {
          target: brave,
          icon: '%SystemRoot%\\System32\\imageres.dll',
          iconIndex: -5
        }
      }
    })
    expect(await resolveIconSource('D:\\Apps\\Brave.lnk', d)).toEqual({
      file: 'C:\\Windows\\System32\\imageres.dll',
      index: -5
    })
  })

  it('falls back to the shortcut target when it has no icon', async () => {
    const d = deps({ files: [brave], links: { 'D:\\Brave.lnk': { target: brave, icon: '' } } })
    expect(await resolveIconSource('D:\\Brave.lnk', d)).toEqual({ file: brave, index: 0 })
  })

  it('falls back to the target when the icon file is missing', async () => {
    const d = deps({
      files: [brave],
      links: { 'D:\\Brave.lnk': { target: brave, icon: 'C:\\gone.ico', iconIndex: 2 } }
    })
    expect(await resolveIconSource('D:\\Brave.lnk', d)).toEqual({ file: brave, index: 0 })
  })

  it('gives up on unreadable or targetless shortcuts (e.g. Store apps)', async () => {
    expect(await resolveIconSource('D:\\Broken.lnk', deps({}))).toBeNull()
    const d = deps({ links: { 'D:\\Store.lnk': { target: '' } } })
    expect(await resolveIconSource('D:\\Store.lnk', d)).toBeNull()
  })

  it('uses a .url file’s IconFile', async () => {
    const ico = 'C:\\Steam\\games\\kr6.ico'
    const d = deps({
      files: [ico],
      readFile: async () =>
        `[InternetShortcut]\nURL=steam://rungameid/1\nIconFile=${ico}\nIconIndex=0`
    })
    expect(await resolveIconSource('D:\\Games\\Kingdom Rush.url', d)).toEqual({
      file: ico,
      index: 0
    })
  })

  it('returns null for a .url without an icon, and for plain files', async () => {
    const d = deps({ readFile: async () => '[InternetShortcut]\nURL=https://example.com' })
    expect(await resolveIconSource('D:\\Site.url', d)).toBeNull()
    expect(await resolveIconSource('D:\\notes.txt', deps({ files: ['D:\\notes.txt'] }))).toBeNull()
  })

  it('uses an .exe’s own icon', async () => {
    expect(await resolveIconSource(brave, deps({ files: [brave] }))).toEqual({
      file: brave,
      index: 0
    })
  })
})
