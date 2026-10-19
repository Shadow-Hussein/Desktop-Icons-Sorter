import { mkdtempSync, readFileSync, existsSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  appSettingsPatchSchema,
  ConfigStore,
  createPanel,
  folderNameFromTitle,
  panelPatchSchema,
  parseConfig
} from '../src/main/store'
import type { AppConfig } from '../src/shared/types'

const bounds = { x: 10, y: 20, width: 300, height: 200 }

describe('parseConfig', () => {
  it('fills defaults for a minimal panel', () => {
    const { config, recovered } = parseConfig({
      panels: [{ id: 'a', title: 'Games', bounds, folderPath: 'C:\\x' }]
    })
    expect(recovered).toBe(false)
    expect(config.version).toBe(2)
    expect(config.panels[0]).toMatchObject({
      collapsed: false,
      collapseDirection: 'up',
      headerShape: 'default',
      collapseAlign: 'start',
      stackable: true,
      bgColor: '#1e1e1e',
      bgOpacity: 0.45,
      iconSize: 'medium',
      sortBy: 'custom',
      cells: [],
      order: [],
      catchNewItems: false
    })
  })

  it('switches name-sorted panels from version 1 to custom order, keeping other sorts', () => {
    const panel = { title: 'x', bounds, folderPath: 'C:\\x' }
    const { config } = parseConfig({
      version: 1,
      panels: [
        { ...panel, id: 'a', sortBy: 'name' },
        { ...panel, id: 'b', sortBy: 'type' }
      ]
    })
    expect(config.version).toBe(2)
    expect(config.panels.map((p) => p.sortBy)).toEqual(['custom', 'type'])
  })

  it('leaves a version 2 config as it is', () => {
    const raw: Partial<AppConfig> = {
      version: 2,
      panels: [createPanel({ title: 'x', bounds, folderPath: 'C:\\x' })]
    }
    raw.panels![0].sortBy = 'name'
    expect(parseConfig(raw).config.panels[0].sortBy).toBe('name')
  })

  it('rejects bad spots', () => {
    const panel = { id: 'a', title: 'x', bounds, folderPath: 'C:\\x' }
    const ok = [{ name: 'a.lnk', col: 2, row: 5 }]
    expect(parseConfig({ panels: [{ ...panel, cells: ok }] }).config.panels[0].cells).toEqual(ok)
    for (const bad of [
      [{ name: 'a', col: -1, row: 0 }],
      [{ name: 'a', col: 1.5, row: 0 }],
      [{ name: '', col: 0, row: 0 }],
      [{ name: 'a', col: 0 }]
    ]) {
      expect(parseConfig({ panels: [{ ...panel, cells: bad }] }).config.panels).toEqual([])
    }
  })

  it('rejects a bad custom order', () => {
    const panel = { id: 'a', title: 'x', bounds, folderPath: 'C:\\x' }
    expect(parseConfig({ panels: [{ ...panel, order: [''] }] }).config.panels).toEqual([])
    expect(parseConfig({ panels: [{ ...panel, order: 'a' }] }).config.panels).toEqual([])
  })

  it('drops only the invalid panels', () => {
    const { config, recovered } = parseConfig({
      locked: true,
      panels: [
        { id: 'ok', title: 'Fine', bounds, folderPath: '/tmp/a' },
        { id: 'bad', title: 'Broken', bounds, folderPath: '/tmp/b', bgOpacity: 7 }
      ]
    })
    expect(recovered).toBe(true)
    expect(config.locked).toBe(true)
    expect(config.panels.map((p) => p.id)).toEqual(['ok'])
  })

  it('turns snapping and stacking on by default', () => {
    const { config } = parseConfig({ panels: [] })
    expect(config).toMatchObject({ snapping: true, stacking: true })
  })

  it('copies added items by default and rejects an unknown mode', () => {
    expect(parseConfig({ panels: [] }).config.addMode).toBe('copy')
    expect(parseConfig({ panels: [], addMode: 'move' }).config.addMode).toBe('move')
    const bad = parseConfig({ panels: [], addMode: 'link' })
    expect(bad.recovered).toBe(true)
    expect(bad.config.addMode).toBe('copy')
  })

  it('falls back to defaults for garbage', () => {
    expect(parseConfig('nope').config.panels).toEqual([])
    expect(parseConfig(null).recovered).toBe(true)
  })
})

describe('appSettingsPatchSchema', () => {
  it('accepts app settings without filling in the rest', () => {
    expect(appSettingsPatchSchema.parse({ snapping: false })).toEqual({ snapping: false })
    expect(appSettingsPatchSchema.parse({ stacking: true, locked: true })).toEqual({
      stacking: true,
      locked: true
    })
  })

  it('rejects other fields and wrong types', () => {
    expect(() => appSettingsPatchSchema.parse({ panels: [] })).toThrow()
    expect(() => appSettingsPatchSchema.parse({ snapping: 'yes' })).toThrow()
    expect(() => appSettingsPatchSchema.parse({ addMode: 'link' })).toThrow()
  })

  it('accepts the add mode', () => {
    expect(appSettingsPatchSchema.parse({ addMode: 'move' })).toEqual({ addMode: 'move' })
  })
})

describe('panelPatchSchema', () => {
  it('accepts customization fields', () => {
    expect(panelPatchSchema.parse({ bgColor: '#ff0000', bgOpacity: 0.2 })).toEqual({
      bgColor: '#ff0000',
      bgOpacity: 0.2
    })
  })

  it('rejects fields the renderer may not set', () => {
    expect(() => panelPatchSchema.parse({ folderPath: 'C:\\Windows' })).toThrow()
    expect(() => panelPatchSchema.parse({ bgColor: 'red' })).toThrow()
    expect(() => panelPatchSchema.parse({ bgOpacity: 1.5 })).toThrow()
    expect(() => panelPatchSchema.parse({ collapseDirection: 'sideways' })).toThrow()
    expect(() => panelPatchSchema.parse({ headerShape: 'round' })).toThrow()
    expect(() => panelPatchSchema.parse({ collapseAlign: 'middle' })).toThrow()
    expect(() => panelPatchSchema.parse({ stackable: 'no' })).toThrow()
    expect(() => panelPatchSchema.parse({ catchNewItems: 1 })).toThrow()
    expect(() => panelPatchSchema.parse({ order: ['a'] })).toThrow()
    expect(() => panelPatchSchema.parse({ sortBy: 'random' })).toThrow()
  })

  it('accepts custom order and catching new items', () => {
    expect(panelPatchSchema.parse({ sortBy: 'custom' })).toEqual({ sortBy: 'custom' })
    expect(panelPatchSchema.parse({ catchNewItems: true })).toEqual({ catchNewItems: true })
  })

  it('accepts a collapse direction', () => {
    expect(panelPatchSchema.parse({ collapseDirection: 'down' })).toEqual({
      collapseDirection: 'down'
    })
  })

  it('accepts a header shape', () => {
    expect(panelPatchSchema.parse({ headerShape: 'phone' })).toEqual({ headerShape: 'phone' })
    expect(panelPatchSchema.parse({ headerShape: 'folder' })).toEqual({ headerShape: 'folder' })
    expect(panelPatchSchema.parse({ collapseAlign: 'center' })).toEqual({ collapseAlign: 'center' })
  })
})

describe('folderNameFromTitle', () => {
  it('strips characters Windows forbids', () => {
    expect(folderNameFromTitle('Work: "Q3" <stuff>?')).toBe('Work Q3 stuff')
  })
  it('handles empty and reserved names', () => {
    expect(folderNameFromTitle('  ...  ')).toBe('Panel')
    expect(folderNameFromTitle('CON')).toBe('CON_')
  })
})

describe('ConfigStore', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'sorter-store-'))
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  it('starts empty when no file exists, then persists updates', async () => {
    const file = join(dir, 'config.json')
    const store = new ConfigStore(file, 0)
    expect(store.load().panels).toEqual([])

    const panel = createPanel({ title: 'Apps', bounds, folderPath: join(dir, 'Apps') })
    store.update((c) => void c.panels.push(panel))
    store.updatePanel(panel.id, (p) => (p.collapsed = true))
    await store.flush()

    const reloaded = new ConfigStore(file).load()
    expect(reloaded.panels).toHaveLength(1)
    expect(reloaded.panels[0].collapsed).toBe(true)
  })

  it('backs up a corrupt file and recovers', () => {
    const file = join(dir, 'config.json')
    writeFileSync(file, '{ not json')
    const config = new ConfigStore(file).load()
    expect(config.panels).toEqual([])
    expect(readFileSync(join(dir, 'config.bad.json'), 'utf8')).toBe('{ not json')
    expect(JSON.parse(readFileSync(file, 'utf8')).version).toBe(2)
  })

  it('notifies listeners and does not mutate previous snapshots', () => {
    const store = new ConfigStore(join(dir, 'c.json'), 10_000)
    store.load()
    const before = store.get()
    let calls = 0
    store.onChange(() => calls++)
    store.update((c) => (c.locked = true))
    expect(calls).toBe(1)
    expect(before.locked).toBe(false)
    expect(store.get().locked).toBe(true)
    store.flushSync()
    expect(existsSync(join(dir, 'c.json'))).toBe(true)
  })
})
