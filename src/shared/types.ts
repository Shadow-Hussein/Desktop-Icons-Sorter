export type IconSize = 'small' | 'medium' | 'large'
/** 'custom' keeps the order the user arranged by dragging (PanelConfig.order). */
export type SortBy = 'custom' | 'name' | 'type' | 'modified'
export type CollapseDirection = 'up' | 'down' | 'left' | 'right'
export type HeaderShape = 'default' | 'phone' | 'folder'
/** How the title bar looks, on any shape. 'soft' fades the bar out instead of hard edges. */
export type TitleBarStyle = 'standard' | 'pill' | 'minimal' | 'soft'
export type CollapseAlign = 'start' | 'center' | 'end'
/** How a panel takes in items from outside: copies them, or moves them out of where they were. */
export type AddMode = 'copy' | 'move'
export type ResizeEdge = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw'

/** An item the user put in a spot of a panel's icon grid, counted from the top left. */
export interface PlacedItem {
  /** File name. */
  name: string
  col: number
  row: number
}

/** Where a drop from outside lands: a cell (in reading order) of a grid `cols` wide. */
export interface DropTarget {
  cell: number
  cols: number
}

export interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

export interface PanelConfig {
  id: string
  title: string
  /** Expanded bounds. When collapsed, the window height shrinks to the title bar only. */
  bounds: Bounds
  collapsed: boolean
  /** Which edge the title bar sits on; the panel collapses toward it. */
  collapseDirection: CollapseDirection
  /**
   * What the panel collapses to: its title bar, a card with a row of its apps
   * (phone, "Dock") or a square with four of them (folder, "Tile").
   */
  headerShape: HeaderShape
  /**
   * How the title bar looks: standard, a capsule inset from the edges (pill),
   * none until the panel collapses (minimal), or fading out at its ends and
   * into the panel instead of hard edges (soft).
   */
  titleBarStyle: TitleBarStyle
  /**
   * Where a collapsed folder square sits along its title bar's edge: the left,
   * middle or right of it (top, middle or bottom for side bars).
   */
  collapseAlign: CollapseAlign
  /**
   * Takes part in stacking (see AppConfig.stacking): collapsing it moves the
   * panels stacked against it, and it moves with a panel it is stacked against.
   */
  stackable: boolean
  /** Background color as #rrggbb. */
  bgColor: string
  /** Background opacity 0–1. Icons and text are not affected. */
  bgOpacity: number
  titleBarColor: string
  titleBarOpacity: number
  titleTextColor: string
  /** Real folder whose contents this panel shows. */
  folderPath: string
  iconSize: IconSize
  sortBy: SortBy
  /**
   * Where the user put items in custom order (sortBy 'custom'), gaps and all.
   * Items without a spot follow the last occupied one, in `order`.
   */
  cells: PlacedItem[]
  /** File names in the order they arrived, for items without a spot in `cells`. */
  order: string[]
  /** New items that appear on the user's Desktop move into this panel. At most one panel has it. */
  catchNewItems: boolean
}

export interface AppConfig {
  /** 1: before custom order existed (see migrateConfig in store.ts). */
  version: 1 | 2
  panels: PanelConfig[]
  startWithWindows: boolean
  /** When locked, panels cannot be moved or resized. */
  locked: boolean
  /** Moving or resizing a panel snaps its edges to screen edges and other panels. */
  snapping: boolean
  /**
   * Collapsing a panel moves the panels stacked against its far edge (below
   * it, for one that collapses up) to fill the space, and expanding moves
   * them back.
   */
  stacking: boolean
  /**
   * Whether items dropped on a panel or caught from the desktop are copied
   * into its folder or moved there. Items from another panel always move.
   */
  addMode: AddMode
}

/** App-wide settings, shown in the app settings window. */
export type AppSettings = Pick<
  AppConfig,
  'startWithWindows' | 'locked' | 'snapping' | 'stacking' | 'addMode'
>

/** Fields of a panel the renderer is allowed to change directly. */
export type PanelPatch = Partial<
  Pick<
    PanelConfig,
    | 'title'
    | 'bgColor'
    | 'bgOpacity'
    | 'titleBarColor'
    | 'titleBarOpacity'
    | 'titleTextColor'
    | 'iconSize'
    | 'sortBy'
    | 'collapseDirection'
    | 'headerShape'
    | 'titleBarStyle'
    | 'collapseAlign'
    | 'stackable'
    | 'catchNewItems'
  >
>

export interface PanelItem {
  /** File name on disk, including extension. */
  fileName: string
  /** Name shown under the icon (.lnk/.url extensions hidden, like Explorer). */
  displayName: string
  path: string
  isDirectory: boolean
  /** .lnk/.url/.pif: drawn with the shortcut arrow, like on the desktop. */
  isShortcut: boolean
  modifiedMs: number
}

export interface PanelState {
  panel: PanelConfig
  locked: boolean
  /** Stacking is on app-wide (AppConfig.stacking). */
  stacking: boolean
  /** How the panel takes in dropped items (AppConfig.addMode). */
  addMode: AddMode
}

export const TITLE_BAR_HEIGHT = 30
/** How long a panel takes to collapse or expand. */
export const COLLAPSE_ANIMATION_MS = 200
/** The row of apps under a collapsed phone panel's title: a 32 px icon plus padding. */
export const PHONE_PEEK_SIZE = 48
/** A collapsed folder panel's square: the title bar over a 2 x 2 grid of 36 px apps. */
export const FOLDER_CARD_SIZE = 122
export const MIN_PANEL_WIDTH = 140
export const MIN_PANEL_HEIGHT = 90
/** Most names a panel's saved order or spots keep, so config.json stays small and valid. */
export const MAX_PANEL_ITEMS = 10_000

export const ICON_PIXELS: Record<IconSize, number> = {
  small: 32,
  medium: 48,
  large: 64
}

/** How far a collapsed panel reaches from its title bar's edge. */
export function collapsedBarSize(shape: HeaderShape): number {
  if (shape === 'folder') return FOLDER_CARD_SIZE
  return TITLE_BAR_HEIGHT + (shape === 'phone' ? PHONE_PEEK_SIZE : 0)
}

/** A collapsed panel's length along its title bar, or null to keep the panel's own. */
export function collapsedLength(shape: HeaderShape): number | null {
  return shape === 'folder' ? FOLDER_CARD_SIZE : null
}

export const IPC = {
  getState: 'panel:get-state',
  stateChanged: 'panel:state-changed',
  updatePanel: 'panel:update',
  toggleCollapsed: 'panel:toggle-collapsed',
  moveStart: 'panel:move-start',
  resizeStart: 'panel:resize-start',
  pointerMove: 'panel:pointer-move',
  pointerEnd: 'panel:pointer-end',
  titleMenu: 'panel:title-menu',
  beginTitleEdit: 'panel:begin-title-edit',

  listItems: 'items:list',
  itemsChanged: 'items:changed',
  getIcon: 'items:get-icon',
  getShortcutOverlay: 'items:get-shortcut-overlay',
  openItem: 'items:open',
  dropFiles: 'items:drop-files',
  arrangeItems: 'items:arrange',
  renameItem: 'items:rename',
  itemMenu: 'items:menu',
  beginItemRename: 'items:begin-rename',
  startDrag: 'items:start-drag',
  backgroundMenu: 'items:background-menu',

  openSettings: 'settings:open',
  settingsChooseFolder: 'settings:choose-folder',
  settingsDeletePanel: 'settings:delete-panel',

  getAppSettings: 'app:get-settings',
  appSettingsChanged: 'app:settings-changed',
  updateAppSettings: 'app:update-settings'
} as const

/** API exposed to renderers as `window.sorter` by the preload script. */
export interface SorterApi {
  /** Empty in the app settings window, which belongs to no panel. */
  panelId: string
  view: 'panel' | 'settings' | 'app'
  getState(): Promise<PanelState>
  onStateChanged(cb: (state: PanelState) => void): () => void
  updatePanel(patch: PanelPatch): Promise<void>
  toggleCollapsed(): Promise<void>

  /** Window move/resize driven by the cursor position the main process reads. */
  moveStart(): void
  resizeStart(edge: ResizeEdge): void
  pointerMove(): void
  pointerEnd(): void

  showTitleMenu(): void
  onBeginTitleEdit(cb: () => void): () => void

  listItems(): Promise<PanelItem[]>
  onItemsChanged(cb: (items: PanelItem[]) => void): () => void
  getIcon(path: string): Promise<string | null>
  /** The shortcut-arrow overlay at this panel's icon size (null off Windows). */
  getShortcutOverlay(): Promise<string | null>
  openItem(path: string): Promise<void>
  /**
   * Copies or moves files (from Explorer, the desktop or another panel) into
   * this panel's folder, into the cell `at` (or after the last item).
   */
  dropFiles(paths: string[], at?: DropTarget | null): Promise<void>
  /** Saves where the user put every item and switches to custom order. */
  arrangeItems(cells: PlacedItem[]): Promise<void>
  renameItem(path: string, newName: string): Promise<void>
  showItemMenu(path: string): void
  showBackgroundMenu(): void
  onBeginItemRename(cb: (path: string) => void): () => void
  startDrag(path: string): void
  getPathForFile(file: File): string

  openSettings(): void
  chooseFolder(): Promise<void>
  deletePanel(): Promise<void>

  /** App settings; only the app settings window may call these. */
  getAppSettings(): Promise<AppSettings>
  onAppSettingsChanged(cb: (settings: AppSettings) => void): () => void
  updateAppSettings(patch: Partial<AppSettings>): Promise<void>
}
