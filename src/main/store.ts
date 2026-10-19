import { mkdirSync, promises as fsp, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { dirname } from 'node:path'
import type {
  AppConfig,
  AppSettings,
  Bounds,
  DropTarget,
  PanelConfig,
  PanelPatch,
  PlacedItem
} from '../shared/types'
import { MAX_PANEL_ITEMS, MIN_PANEL_HEIGHT, MIN_PANEL_WIDTH } from '../shared/types'
import {
  array,
  boolean,
  number,
  object,
  oneOf,
  strictPartial,
  string,
  withDefault
} from './validate'

const hexColor = string({ pattern: /^#[0-9a-fA-F]{6}$/ })
const opacity = number({ min: 0, max: 1 })

export const boundsSchema = object<Bounds>({
  x: number(),
  y: number(),
  width: number({ min: MIN_PANEL_WIDTH }),
  height: number({ min: MIN_PANEL_HEIGHT })
})

const iconSize = oneOf(['small', 'medium', 'large'])
const sortBy = oneOf(['custom', 'name', 'type', 'modified'])
const collapseDirection = oneOf(['up', 'down', 'left', 'right'])
const headerShape = oneOf(['default', 'phone', 'folder', 'pill', 'minimal'])
const collapseAlign = oneOf(['start', 'center', 'end'])
const addMode = oneOf(['copy', 'move'])
const title = string({ max: 200 })
/** A file name, as Windows allows it. */
export const fileNameSchema = string({ min: 1, max: 255 })
export const orderSchema = array(fileNameSchema, { max: MAX_PANEL_ITEMS })
const gridIndex = (max: number) => number({ min: 0, max, integer: true })
export const cellsSchema = array(
  object<PlacedItem>({ name: fileNameSchema, col: gridIndex(999), row: gridIndex(99_999) }),
  { max: MAX_PANEL_ITEMS }
)
export const dropTargetSchema = object<DropTarget>({
  cell: gridIndex(99_999_999),
  cols: number({ min: 1, max: 1000, integer: true })
})

export const panelSchema = object<PanelConfig>({
  id: string({ min: 1 }),
  title,
  bounds: boundsSchema,
  collapsed: withDefault(boolean(), false),
  collapseDirection: withDefault(collapseDirection, 'up'),
  headerShape: withDefault(headerShape, 'default'),
  collapseAlign: withDefault(collapseAlign, 'start'),
  stackable: withDefault(boolean(), true),
  bgColor: withDefault(hexColor, '#1e1e1e'),
  bgOpacity: withDefault(opacity, 0.45),
  titleBarColor: withDefault(hexColor, '#000000'),
  titleBarOpacity: withDefault(opacity, 0.55),
  titleTextColor: withDefault(hexColor, '#ffffff'),
  folderPath: string({ min: 1 }),
  iconSize: withDefault(iconSize, 'medium'),
  sortBy: withDefault(sortBy, 'custom'),
  cells: withDefault(cellsSchema, []),
  order: withDefault(orderSchema, []),
  catchNewItems: withDefault(boolean(), false)
})

/**
 * Fields a renderer may change. Deliberately built without defaults: a
 * partial schema with defaults would fill in (and so reset) unsent fields.
 */
export const panelPatchSchema = strictPartial<PanelPatch>({
  title,
  bgColor: hexColor,
  bgOpacity: opacity,
  titleBarColor: hexColor,
  titleBarOpacity: opacity,
  titleTextColor: hexColor,
  iconSize,
  sortBy,
  collapseDirection,
  headerShape,
  collapseAlign,
  stackable: boolean(),
  catchNewItems: boolean()
})

export const configSchema = object<AppConfig>({
  version: withDefault(oneOf([1, 2]), 2),
  panels: withDefault(array(panelSchema), []),
  startWithWindows: withDefault(boolean(), false),
  locked: withDefault(boolean(), false),
  snapping: withDefault(boolean(), true),
  stacking: withDefault(boolean(), true),
  addMode: withDefault(addMode, 'copy')
})

/** App settings the app settings window may change; no defaults, as for panel patches. */
export const appSettingsPatchSchema = strictPartial<AppSettings>({
  startWithWindows: boolean(),
  locked: boolean(),
  snapping: boolean(),
  stacking: boolean(),
  addMode
})

export function defaultConfig(): AppConfig {
  return {
    version: 2,
    panels: [],
    startWithWindows: false,
    locked: false,
    snapping: true,
    stacking: true,
    addMode: 'copy'
  }
}

/**
 * Parse raw config JSON. Invalid panels are dropped individually so one bad
 * entry does not wipe out the rest; a structurally broken file yields defaults.
 */
export function parseConfig(raw: unknown): { config: AppConfig; recovered: boolean } {
  const full = configSchema.safeParse(raw)
  if (full.success) return { config: migrateConfig(full.data), recovered: false }

  if (raw && typeof raw === 'object') {
    const obj = raw as Record<string, unknown>
    const panels = Array.isArray(obj.panels)
      ? obj.panels.flatMap((p) => {
          const r = panelSchema.safeParse(p)
          return r.success ? [r.data] : []
        })
      : []
    const rest = configSchema.safeParse({ ...obj, panels: [] })
    const base = rest.success ? rest.data : defaultConfig()
    return { config: migrateConfig({ ...base, panels }), recovered: true }
  }
  return { config: defaultConfig(), recovered: true }
}

/**
 * Version 2 made custom order (drag to arrange) the default. Panels saved by
 * version 1 sorted by name switch to it without moving an icon, since an
 * empty custom order sorts by name. Type and date sorts are kept: custom
 * order can't reproduce them before the folder has been listed.
 */
export function migrateConfig(config: AppConfig): AppConfig {
  if (config.version === 2) return config
  return {
    ...config,
    version: 2,
    panels: config.panels.map((p) => (p.sortBy === 'name' ? { ...p, sortBy: 'custom' } : p))
  }
}

export interface NewPanelOptions {
  title: string
  bounds: Bounds
  folderPath: string
}

export function createPanel({ title, bounds, folderPath }: NewPanelOptions): PanelConfig {
  return panelSchema.parse({ id: randomUUID(), title, bounds, folderPath })
}

/** Turn a panel title into a safe Windows folder name. */
export function folderNameFromTitle(title: string): string {
  const cleaned = title
    .replace(/[<>:"/\\|?*]/g, '')
    // eslint-disable-next-line no-control-regex
    .replace(/[\x00-\x1f]/g, '')
    .replace(/[. ]+$/, '')
    .trim()
  const name = cleaned || 'Panel'
  return /^(con|prn|aux|nul|com\d|lpt\d)$/i.test(name) ? `${name}_` : name
}

/**
 * Loads and persists the app config as JSON. Writes are debounced and atomic
 * (write temp file, then rename) so a crash never leaves a half-written file.
 */
export class ConfigStore {
  private config: AppConfig = defaultConfig()
  private timer: NodeJS.Timeout | null = null
  private writing: Promise<void> = Promise.resolve()
  private readonly listeners = new Set<(config: AppConfig) => void>()

  constructor(
    readonly filePath: string,
    private readonly debounceMs = 300
  ) {}

  load(): AppConfig {
    let raw: unknown
    try {
      raw = JSON.parse(readFileSync(this.filePath, 'utf8'))
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        this.config = defaultConfig()
        return this.config
      }
      raw = undefined
    }
    const { config, recovered } = parseConfig(raw)
    if (recovered) {
      try {
        renameSync(this.filePath, this.filePath.replace(/\.json$/, '') + '.bad.json')
      } catch {
        // Keep going with defaults even if the backup fails.
      }
    }
    this.config = config
    if (recovered) this.flushSync()
    return this.config
  }

  get(): AppConfig {
    return this.config
  }

  getPanel(id: string): PanelConfig | undefined {
    return this.config.panels.find((p) => p.id === id)
  }

  update(mutator: (draft: AppConfig) => void): AppConfig {
    const draft = structuredClone(this.config)
    mutator(draft)
    this.config = draft
    this.scheduleSave()
    for (const l of this.listeners) l(this.config)
    return this.config
  }

  updatePanel(id: string, mutator: (panel: PanelConfig) => void): PanelConfig | undefined {
    let result: PanelConfig | undefined
    this.update((draft) => {
      const panel = draft.panels.find((p) => p.id === id)
      if (panel) {
        mutator(panel)
        result = panel
      }
    })
    return result
  }

  onChange(listener: (config: AppConfig) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private scheduleSave(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      void this.flush().catch((err) => console.error('[store] save failed', err))
    }, this.debounceMs)
  }

  /** Writes are chained so two saves never race on the temp file. */
  flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
    const write = async (): Promise<void> => {
      const tmp = this.filePath + '.tmp'
      await fsp.mkdir(dirname(this.filePath), { recursive: true })
      await fsp.writeFile(tmp, JSON.stringify(this.config, null, 2), 'utf8')
      await fsp.rename(tmp, this.filePath)
    }
    this.writing = this.writing.catch(() => undefined).then(write)
    return this.writing
  }

  /** Synchronous save, used on quit when there is no time for async work. */
  flushSync(): void {
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
    const tmp = this.filePath + '.sync.tmp'
    mkdirSync(dirname(this.filePath), { recursive: true })
    writeFileSync(tmp, JSON.stringify(this.config, null, 2), 'utf8')
    renameSync(tmp, this.filePath)
  }
}
