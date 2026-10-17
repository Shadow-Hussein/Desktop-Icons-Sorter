import { mkdtempSync, readFileSync, existsSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  ConfigStore,
  createPanel,
  folderNameFromTitle,
  panelPatchSchema,
  parseConfig
} from '../src/main/store'

const bounds = { x: 10, y: 20, width: 300, height: 200 }

describe('parseConfig', () => {
  it('fills defaults for a minimal panel', () => {
    const { config, recovered } = parseConfig({
      panels: [{ id: 'a', title: 'Games', bounds, folderPath: 'C:\\x' }]
    })
    expect(recovered).toBe(false)
    expect(config.version).toBe(1)
    expect(config.panels[0]).toMatchObject({
      collapsed: false,
      collapseDirection: 'up',
      bgColor: '#1e1e1e',
      bgOpacity: 0.45,
      iconSize: 'medium',
      sortBy: 'name'
    })
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

  it('falls back to defaults for garbage', () => {
    expect(parseConfig('nope').config.panels).toEqual([])
    expect(parseConfig(null).recovered).toBe(true)
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
  })

  it('accepts a collapse direction', () => {
    expect(panelPatchSchema.parse({ collapseDirection: 'down' })).toEqual({
      collapseDirection: 'down'
    })
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
    expect(JSON.parse(readFileSync(file, 'utf8')).version).toBe(1)
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
