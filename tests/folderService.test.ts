import { mkdirSync, mkdtempSync, promises as fsp, rmSync, writeFileSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  copyInto,
  displayNameFor,
  isInside,
  listItems,
  moveInto,
  placeInOrder,
  pruneCells,
  renameInCells,
  renameInOrder,
  renameItem,
  sortItems,
  syncOrder,
  uniqueDestination
} from '../src/main/folderService'
import type { PanelItem } from '../src/shared/types'
import { MAX_PANEL_ITEMS } from '../src/shared/types'

let root: string
beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'sorter-fs-'))
})
afterEach(() => {
  vi.restoreAllMocks()
  rmSync(root, { recursive: true, force: true })
})

const touch = (p: string, content = 'x'): void => writeFileSync(p, content)

describe('displayNameFor', () => {
  it('hides shortcut extensions only', () => {
    expect(displayNameFor('Steam.lnk')).toBe('Steam')
    expect(displayNameFor('Site.URL')).toBe('Site')
    expect(displayNameFor('notes.txt')).toBe('notes.txt')
  })
})

describe('isInside', () => {
  it('detects descendants', () => {
    expect(isInside(join(root, 'a', 'b'), root)).toBe(true)
    expect(isInside(root, root)).toBe(true)
    expect(isInside(join(root, '..', 'other'), root)).toBe(false)
  })
})

describe('listItems', () => {
  it('lists files, hides desktop.ini, and sorts by name', async () => {
    touch(join(root, 'b.txt'))
    touch(join(root, 'A.lnk'))
    touch(join(root, 'desktop.ini'))
    mkdirSync(join(root, 'Folder'))
    const items = await listItems(root, 'name')
    expect(items.map((i) => i.displayName)).toEqual(['A', 'b.txt', 'Folder'])
    expect(items.find((i) => i.fileName === 'Folder')?.isDirectory).toBe(true)
  })

  it('returns [] for a missing folder', async () => {
    expect(await listItems(join(root, 'missing'), 'name')).toEqual([])
  })
})

describe('sortItems', () => {
  const item = (fileName: string, modifiedMs: number, isDirectory = false): PanelItem => ({
    fileName,
    displayName: displayNameFor(fileName),
    path: fileName,
    isDirectory,
    isShortcut: false,
    modifiedMs
  })
  const items = [item('z.txt', 1), item('a.lnk', 3), item('m.exe', 2), item('dir', 0, true)]

  it('sorts by type with folders first', () => {
    expect(sortItems(items, 'type').map((i) => i.fileName)).toEqual([
      'dir',
      'm.exe',
      'a.lnk',
      'z.txt'
    ])
  })
  it('sorts by modified, newest first', () => {
    expect(sortItems(items, 'modified').map((i) => i.fileName)).toEqual([
      'a.lnk',
      'm.exe',
      'z.txt',
      'dir'
    ])
  })
  it('sorts names naturally', () => {
    const n = [item('file10', 0), item('file2', 0)]
    expect(sortItems(n, 'name').map((i) => i.fileName)).toEqual(['file2', 'file10'])
  })
  it('follows the custom order, then the rest by name', () => {
    expect(sortItems(items, 'custom', ['m.exe', 'Z.TXT']).map((i) => i.fileName)).toEqual([
      'm.exe',
      'z.txt',
      'a.lnk',
      'dir'
    ])
  })
  it('reads spots row by row before the custom order', () => {
    const cells = [
      { name: 'dir', col: 2, row: 1 },
      { name: 'a.lnk', col: 3, row: 0 }
    ]
    expect(sortItems(items, 'custom', ['z.txt'], cells).map((i) => i.fileName)).toEqual([
      'a.lnk',
      'dir',
      'z.txt',
      'm.exe'
    ])
  })
  it('sorts by name when there is no custom order yet', () => {
    expect(sortItems(items, 'custom').map((i) => i.fileName)).toEqual(
      sortItems(items, 'name').map((i) => i.fileName)
    )
  })
})

describe('syncOrder', () => {
  it('drops names that are gone and appends new ones', () => {
    expect(syncOrder(['b', 'gone', 'a'], ['b', 'a', 'new'])).toEqual(['b', 'a', 'new'])
  })
  it('returns null when nothing changed', () => {
    expect(syncOrder(['b', 'a'], ['b', 'a'])).toBeNull()
    expect(syncOrder(['b', 'a'], ['a', 'b'])).toBeNull()
  })
})

describe('order cap', () => {
  const many = Array.from({ length: MAX_PANEL_ITEMS + 5 }, (_, i) => `f${i}`)

  it('keeps at most MAX_PANEL_ITEMS names, and is stable once full', () => {
    const order = syncOrder([], many)!
    expect(order).toHaveLength(MAX_PANEL_ITEMS)
    expect(syncOrder(order, many)).toBeNull()
  })

  it('drops the oldest names when placing past the cap', () => {
    const order = placeInOrder(many.slice(0, MAX_PANEL_ITEMS), ['new'], null)
    expect(order).toHaveLength(MAX_PANEL_ITEMS)
    expect(order.at(-1)).toBe('new')
    expect(order[0]).toBe('f1')
  })
})

describe('placeInOrder', () => {
  it('moves names before another one', () => {
    expect(placeInOrder(['a', 'b', 'c', 'd'], ['d'], 'b')).toEqual(['a', 'd', 'b', 'c'])
    expect(placeInOrder(['a', 'b', 'c'], ['a'], 'c')).toEqual(['b', 'a', 'c'])
  })
  it('adds new names at the end without a target, or when the target is unknown', () => {
    expect(placeInOrder(['a', 'b'], ['x', 'y'], null)).toEqual(['a', 'b', 'x', 'y'])
    expect(placeInOrder(['a', 'b'], ['x'], 'nope')).toEqual(['a', 'b', 'x'])
  })
})

describe('pruneCells', () => {
  it('forgets spots of items that are gone', () => {
    const cells = [
      { name: 'a', col: 0, row: 0 },
      { name: 'gone', col: 1, row: 0 }
    ]
    expect(pruneCells(cells, ['A'])).toEqual([{ name: 'a', col: 0, row: 0 }])
    expect(pruneCells(cells, ['a', 'gone'])).toBeNull()
  })
})

describe('renameInCells', () => {
  it('keeps the renamed item in its spot', () => {
    expect(renameInCells([{ name: 'b', col: 2, row: 3 }], 'b', 'bee')).toEqual([
      { name: 'bee', col: 2, row: 3 }
    ])
  })
})

describe('renameInOrder', () => {
  it('keeps the renamed item in place', () => {
    expect(renameInOrder(['a', 'b', 'c'], 'b', 'bee')).toEqual(['a', 'bee', 'c'])
  })
})

describe('uniqueDestination', () => {
  it('appends (n) before the extension', async () => {
    touch(join(root, 'app.lnk'))
    touch(join(root, 'app (2).lnk'))
    expect(await uniqueDestination(root, 'app.lnk')).toBe(join(root, 'app (3).lnk'))
    expect(await uniqueDestination(root, 'new.lnk')).toBe(join(root, 'new.lnk'))
  })
})

describe('copyInto', () => {
  it('copies a file, keeps the original and renames on collision', async () => {
    const desk = join(root, 'Desktop')
    const panel = join(root, 'Panel')
    mkdirSync(desk)
    mkdirSync(panel)
    touch(join(desk, 'Game.lnk'), 'new')
    touch(join(panel, 'Game.lnk'), 'old')

    const dest = await copyInto(join(desk, 'Game.lnk'), panel)
    expect(dest).toBe(join(panel, 'Game (2).lnk'))
    expect(await fsp.readFile(join(desk, 'Game.lnk'), 'utf8')).toBe('new')
    expect(await fsp.readFile(dest, 'utf8')).toBe('new')
  })

  it('copies folders with their contents', async () => {
    mkdirSync(join(root, 'f'))
    touch(join(root, 'f', 'inner.txt'), 'hi')
    const dest = await copyInto(join(root, 'f'), join(root, 'panel'))
    expect(await fsp.readFile(join(dest, 'inner.txt'), 'utf8')).toBe('hi')
    expect(existsSync(join(root, 'f', 'inner.txt'))).toBe(true)
  })

  it('is a no-op when already in the destination', async () => {
    touch(join(root, 'a.txt'))
    expect(await copyInto(join(root, 'a.txt'), root)).toBe(join(root, 'a.txt'))
  })

  it('refuses to copy a folder into itself', async () => {
    mkdirSync(join(root, 'f'))
    await expect(copyInto(join(root, 'f'), join(root, 'f', 'sub'))).rejects.toThrow(/itself/)
  })
})

describe('moveInto', () => {
  it('moves a file and renames on collision', async () => {
    const desk = join(root, 'Desktop')
    const panel = join(root, 'Panel')
    mkdirSync(desk)
    mkdirSync(panel)
    touch(join(desk, 'Game.lnk'), 'new')
    touch(join(panel, 'Game.lnk'), 'old')

    const dest = await moveInto(join(desk, 'Game.lnk'), panel)
    expect(dest).toBe(join(panel, 'Game (2).lnk'))
    expect(existsSync(join(desk, 'Game.lnk'))).toBe(false)
    expect(await fsp.readFile(dest, 'utf8')).toBe('new')
  })

  it('is a no-op when already in the destination', async () => {
    touch(join(root, 'a.txt'))
    expect(await moveInto(join(root, 'a.txt'), root)).toBe(join(root, 'a.txt'))
  })

  it('creates the destination folder', async () => {
    touch(join(root, 'a.txt'))
    const dest = await moveInto(join(root, 'a.txt'), join(root, 'new', 'panel'))
    expect(existsSync(dest)).toBe(true)
  })

  it('refuses to move a folder into itself', async () => {
    mkdirSync(join(root, 'f'))
    await expect(moveInto(join(root, 'f'), join(root, 'f', 'sub'))).rejects.toThrow(/itself/)
  })

  it('falls back to copy + delete across drives (EXDEV)', async () => {
    const src = join(root, 'src')
    mkdirSync(src)
    mkdirSync(join(src, 'Tools'))
    touch(join(src, 'Tools', 'tool.exe'), 'bin')
    const err = Object.assign(new Error('cross-device'), { code: 'EXDEV' })
    vi.spyOn(fsp, 'rename').mockRejectedValueOnce(err)

    const dest = await moveInto(join(src, 'Tools'), join(root, 'dst'))
    expect(await fsp.readFile(join(dest, 'tool.exe'), 'utf8')).toBe('bin')
    expect(existsSync(join(src, 'Tools'))).toBe(false)
  })
})

describe('renameItem', () => {
  it('keeps the hidden .lnk extension when the user types a display name', async () => {
    touch(join(root, 'Old.lnk'))
    expect(await renameItem(join(root, 'Old.lnk'), 'New')).toBe(join(root, 'New.lnk'))
    expect(existsSync(join(root, 'New.lnk'))).toBe(true)
  })

  it('rejects invalid names and existing targets', async () => {
    touch(join(root, 'a.txt'))
    touch(join(root, 'b.txt'))
    await expect(renameItem(join(root, 'a.txt'), 'bad:name')).rejects.toThrow(/not a valid/)
    await expect(renameItem(join(root, 'a.txt'), 'b.txt')).rejects.toThrow(/already exists/)
  })
})

describe('isShortcut', () => {
  it('flags .lnk, .url and .pif items only', async () => {
    touch(join(root, 'Steam.lnk'))
    touch(join(root, 'Game.URL'))
    touch(join(root, 'tool.exe'))
    mkdirSync(join(root, 'Folder.lnk')) // a folder that merely looks like a shortcut
    const items = await listItems(root, 'name')
    const flags = Object.fromEntries(items.map((i) => [i.fileName, i.isShortcut]))
    expect(flags).toEqual({
      'Steam.lnk': true,
      'Game.URL': true,
      'tool.exe': false,
      'Folder.lnk': false
    })
  })
})
