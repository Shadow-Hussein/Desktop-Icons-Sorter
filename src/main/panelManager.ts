import {
  BrowserWindow,
  Menu,
  app,
  dialog,
  nativeImage,
  screen,
  shell,
  systemPreferences,
  type MenuItemConstructorOptions,
  type WebPreferences
} from 'electron'
import { existsSync, promises as fsp } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import type {
  AppSettings,
  Bounds,
  CollapseDirection,
  DropTarget,
  IconSize,
  PanelConfig,
  PanelPatch,
  PanelState,
  PlacedItem,
  ResizeEdge,
  SortBy
} from '../shared/types'
import {
  COLLAPSE_ANIMATION_MS,
  collapsedBarSize,
  collapsedLength,
  ICON_PIXELS,
  IPC,
  MAX_PANEL_ITEMS
} from '../shared/types'
import { attachToDesktop, needsReattach, sendToBottom } from './desktopLayer'
import { DesktopCatcher } from './desktopCatcher'
import {
  copyInto,
  isInside,
  listItems,
  moveInto,
  placeInOrder,
  renameInOrder,
  renameItem,
  samePath,
  pruneCells,
  renameInCells,
  syncOrder
} from './folderService'
import { layoutGrid, placeAt } from '../shared/grid'
import {
  clampToWorkAreas,
  collapsedBounds,
  expandedFromCollapsed,
  isSideways,
  resizeBounds,
  snapLines,
  snapMove,
  snapResize,
  visualBounds,
  type SnapLines
} from './geometry'
import { getIconDataUrl } from './icons'
import { getShortcutOverlayDataUrl } from './shortcutOverlay'
import { stackMove } from './stack'
import { ConfigStore, createPanel, folderNameFromTitle } from './store'
import { createTrayIcon } from './trayIcon'
import { FolderWatcher } from './watcher'

export interface PanelManagerOptions {
  store: ConfigStore
  /** The user's Desktop folder; items are moved back here when leaving a panel. */
  desktopDir: string
  /** Parent folder for new panels' folders. */
  panelsRootDir: string
  preloadPath: string
  /** Loads the renderer into a window with the given query parameters. */
  loadRenderer(win: BrowserWindow, query: Record<string, string>): Promise<void>
}

interface PointerSession {
  kind: 'move' | 'resize'
  edge?: ResizeEdge
  startCursor: { x: number; y: number }
  startBounds: Bounds
  /** The bounds last asked for. Saved at the end instead of the window's, which Windows may have rounded. */
  last: Bounds
  /** What the panel covers at the start, which is what snaps (see visualBounds). */
  startVisual: Bounds
  /** Other panels' edges, or null when snapping is off. */
  lines: SnapLines | null
}

interface PanelRuntime {
  win: BrowserWindow
  watcher: FolderWatcher
  pointer: PointerSession | null
  settingsWin: BrowserWindow | null
  /** Shrinks the window once the renderer's collapse animation is over. */
  collapseTimer: NodeJS.Timeout | null
  /** Moves the window along with a panel in its stack that collapses or expands. */
  slide: NodeJS.Timeout | null
}

/**
 * Windows won't make a transparent window much shorter than 39 px. 40 is
 * also a whole number of pixels at 125%, 150% and 175% scaling.
 */
const MIN_WINDOW_DEPTH = 40

/**
 * All windows share one renderer process (see index.ts), so a crash blanks
 * every panel and the settings window at once. Reload, but at most every
 * 10 s so a crash loop can't spin.
 */
function reloadOnCrash(win: BrowserWindow): void {
  let lastCrash = 0
  win.webContents.on('render-process-gone', (_e, { reason }) => {
    if (reason === 'clean-exit' || win.isDestroyed() || Date.now() - lastCrash < 10_000) return
    lastCrash = Date.now()
    win.reload()
  })
}

/** Login items exist only on Windows and macOS. */
export function canStartWithOs(): boolean {
  return process.platform === 'win32' || process.platform === 'darwin'
}

/**
 * The portable build runs from a temp folder it unpacks into, which is gone
 * after it quits, so its login item has to point at the portable exe itself.
 */
export function setStartWithOs(open: boolean): void {
  const portable = process.env.PORTABLE_EXECUTABLE_FILE
  app.setLoginItemSettings({ openAtLogin: open, ...(portable ? { path: portable } : {}) })
}

export class PanelManager {
  private readonly panels = new Map<string, PanelRuntime>()
  /** webContents id → panel id, for both panel and settings windows. */
  private readonly senders = new Map<number, string>()
  private hidden = false
  private reattachTimer: NodeJS.Timeout | null = null
  private appWin: { win: BrowserWindow; webContentsId: number } | null = null
  /** Moves new desktop items into the panel that catches them; null when none does. */
  private catcher: DesktopCatcher | null = null

  constructor(private readonly opts: PanelManagerOptions) {}

  get store(): ConfigStore {
    return this.opts.store
  }

  // ---------------------------------------------------------------- lifecycle

  async start(): Promise<void> {
    if (this.store.get().panels.length === 0) this.addPanel('Apps')
    for (const p of this.store.get().panels) await this.openPanel(p)
    this.syncCatcher()

    screen.on('display-removed', () => this.clampAll())
    screen.on('display-metrics-changed', () => this.clampAll())

    // Explorer restarts recreate Progman; re-own our windows when that happens.
    this.reattachTimer = setInterval(() => {
      for (const rt of this.panels.values()) if (needsReattach(rt.win)) attachToDesktop(rt.win)
    }, 10_000)
  }

  stop(): void {
    if (this.reattachTimer) clearInterval(this.reattachTimer)
    for (const rt of this.panels.values()) rt.watcher.close()
    this.catcher?.stop()
  }

  panelIdForSender(webContentsId: number): string | undefined {
    return this.senders.get(webContentsId)
  }

  /** The same locked-down preferences for every window. */
  private webPreferences(): WebPreferences {
    return {
      preload: this.opts.preloadPath,
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      spellcheck: false
    }
  }

  /** Packaged, windows take the app exe's icon. Run from source they would
      take electron.exe's, so point them at the build icon instead. */
  private windowIcon(): string | undefined {
    return app.isPackaged ? undefined : join(app.getAppPath(), 'build', 'icon.ico')
  }

  private windowBounds(p: PanelConfig): Bounds {
    return p.collapsed
      ? collapsedBounds(
          p.bounds,
          p.collapseDirection,
          // Ask for at least Windows' minimum up front (see placeWindow), so a
          // collapsing panel doesn't visibly jump while placeWindow corrects it.
          Math.max(collapsedBarSize(p.headerShape), MIN_WINDOW_DEPTH),
          collapsedLength(p.headerShape),
          p.collapseAlign
        )
      : p.bounds
  }

  /**
   * Windows won't make a transparent window shorter than about 38 px, which is
   * taller than a collapsed title bar, and rounds sizes at fractional scaling.
   * When the bar sits at the bottom or right, keep that edge where it belongs
   * and let the extra size go the other way.
   */
  private placeWindow(win: BrowserWindow, target: Bounds, barSide: CollapseDirection | null): void {
    win.setBounds(target)
    if (barSide !== 'down' && barSide !== 'right') return
    const got = win.getBounds()
    const x = barSide === 'right' ? target.x + target.width - got.width : got.x
    const y = barSide === 'down' ? target.y + target.height - got.height : got.y
    if (x !== got.x || y !== got.y) win.setBounds({ ...got, x, y })
  }

  private applyBounds(win: BrowserWindow, p: PanelConfig): void {
    this.placeWindow(win, this.windowBounds(p), p.collapsed ? p.collapseDirection : null)
  }

  private async openPanel(panel: PanelConfig): Promise<void> {
    const workAreas = screen.getAllDisplays().map((d) => d.workArea)
    const bounds = clampToWorkAreas(panel.bounds, workAreas, panel.collapseDirection)
    if (bounds !== panel.bounds) this.store.updatePanel(panel.id, (p) => (p.bounds = bounds))
    const current = this.store.getPanel(panel.id)!

    await fsp.mkdir(current.folderPath, { recursive: true }).catch(() => undefined)

    const win = new BrowserWindow({
      ...this.windowBounds(current),
      frame: false,
      transparent: true,
      backgroundColor: '#00000000',
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      skipTaskbar: true,
      hasShadow: false,
      show: false,
      title: current.title,
      // WS_EX_TOOLWINDOW on Windows: keeps panels out of Alt+Tab.
      ...(process.platform === 'win32' ? { type: 'toolbar' } : {}),
      webPreferences: this.webPreferences()
    })
    this.applyBounds(win, current)
    const rt: PanelRuntime = {
      win,
      watcher: new FolderWatcher(() => void this.pushItems(panel.id)),
      pointer: null,
      settingsWin: null,
      collapseTimer: null,
      slide: null
    }
    this.panels.set(panel.id, rt)
    const wcId = win.webContents.id
    this.senders.set(wcId, panel.id)

    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    win.webContents.on('will-navigate', (e) => e.preventDefault())
    win.once('ready-to-show', () => {
      if (!this.hidden) win.showInactive()
      attachToDesktop(win)
    })
    // A panel that joins the shared renderer process (see index.ts) gets an
    // opaque white page background, so re-apply transparency on every load.
    win.webContents.on('did-finish-load', () => win.setBackgroundColor('#00000000'))
    win.on('blur', () => sendToBottom(win))
    win.on('closed', () => {
      if (rt.collapseTimer) clearTimeout(rt.collapseTimer)
      if (rt.slide) clearTimeout(rt.slide)
      this.senders.delete(wcId)
      rt.watcher.close()
      rt.settingsWin?.close()
    })
    reloadOnCrash(win)

    rt.watcher.watch(current.folderPath)
    await this.opts.loadRenderer(win, { view: 'panel', panel: panel.id })
  }

  private clampAll(): void {
    const workAreas = screen.getAllDisplays().map((d) => d.workArea)
    for (const p of this.store.get().panels) {
      const b = clampToWorkAreas(p.bounds, workAreas, p.collapseDirection)
      if (b !== p.bounds) {
        const updated = this.store.updatePanel(p.id, (d) => (d.bounds = b))
        const rt = this.panels.get(p.id)
        if (updated && rt) this.applyBounds(rt.win, updated)
      }
    }
  }

  // ------------------------------------------------------------ panel config

  getState(panelId: string): PanelState | undefined {
    const panel = this.store.getPanel(panelId)
    const { locked, stacking, addMode } = this.store.get()
    return panel && { panel, locked, stacking, addMode }
  }

  private pushState(panelId: string): void {
    const state = this.getState(panelId)
    const rt = this.panels.get(panelId)
    if (!state || !rt) return
    for (const w of [rt.win, rt.settingsWin]) {
      if (w && !w.isDestroyed()) w.webContents.send(IPC.stateChanged, state)
    }
  }

  private pushStateAll(): void {
    for (const id of this.panels.keys()) this.pushState(id)
  }

  updatePanel(panelId: string, patch: PanelPatch): void {
    const before = this.store.getPanel(panelId)
    const updated = this.store.updatePanel(panelId, (p) => Object.assign(p, patch))
    if (!updated || !before) return
    const rt = this.panels.get(panelId)
    if (rt && patch.title !== undefined) {
      rt.win.setTitle(updated.title)
      rt.settingsWin?.setTitle(`Panel settings: ${updated.title}`)
    }
    this.pushState(panelId)
    if (
      rt &&
      updated.collapsed &&
      (updated.collapseDirection !== before.collapseDirection ||
        updated.headerShape !== before.headerShape ||
        updated.collapseAlign !== before.collapseAlign)
    ) {
      this.applyBounds(rt.win, updated)
    }
    if (patch.sortBy !== undefined && patch.sortBy !== before.sortBy) void this.pushItems(panelId)
    if (patch.catchNewItems !== undefined) {
      // Only one panel catches new desktop items.
      if (patch.catchNewItems) {
        const others = this.store.get().panels.filter((o) => o.id !== panelId && o.catchNewItems)
        this.store.update((c) => {
          for (const o of c.panels) if (o.id !== panelId) o.catchNewItems = false
        })
        for (const o of others) this.pushState(o.id)
      }
      this.syncCatcher()
    }
  }

  /** Watches the desktop only while a panel catches new items. */
  private syncCatcher(): void {
    const catching = this.store.get().panels.some((p) => p.catchNewItems)
    if (catching && !this.catcher) {
      this.catcher = new DesktopCatcher(this.opts.desktopDir, (path) => this.catchItem(path))
      this.catcher.start()
    } else if (!catching && this.catcher) {
      this.catcher.stop()
      this.catcher = null
    }
  }

  /** Copies or moves (see AppConfig.addMode) a new desktop item to the end of the catching panel. */
  private async catchItem(path: string): Promise<boolean> {
    const p = this.store.get().panels.find((x) => x.catchNewItems)
    // A panel folder the user put on the desktop is not a new item.
    if (!p || this.store.get().panels.some((x) => isInside(x.folderPath, path))) return true
    try {
      const add = this.store.get().addMode === 'copy' ? copyInto : moveInto
      const dest = await add(path, p.folderPath)
      this.store.updatePanel(p.id, (d) => (d.order = placeInOrder(d.order, [basename(dest)], null)))
      return true
    } catch (err) {
      // Locked, or on a path that needs admin rights; the catcher may retry.
      console.warn('[catcher] could not add', path, (err as Error).message)
      return false
    }
  }

  toggleCollapsed(panelId: string): void {
    const before = this.store.getPanel(panelId)
    const updated = this.store.updatePanel(panelId, (p) => (p.collapsed = !p.collapsed))
    const rt = this.panels.get(panelId)
    if (!before || !updated || !rt) return
    // The renderer animates the panel with a CSS transition inside a window
    // that holds the expanded size, which is far smoother than resizing the
    // window every frame. So the window grows before expanding and shrinks
    // only after collapsing has finished.
    if (rt.collapseTimer) clearTimeout(rt.collapseTimer)
    rt.collapseTimer = null
    if (updated.collapsed) {
      this.pushState(panelId)
      rt.collapseTimer = setTimeout(() => this.finishCollapse(panelId), COLLAPSE_ANIMATION_MS)
    } else {
      this.applyBounds(rt.win, updated)
      this.pushState(panelId)
    }
    if (this.store.get().stacking) this.moveStack(before, updated)
  }

  /** Moves the panels stacked against a panel that collapsed or expanded (see stackMove). */
  private moveStack(before: PanelConfig, after: PanelConfig): void {
    const move = stackMove(before, after, this.store.get().panels)
    // Leave alone a panel the user is dragging right now.
    const ids = move.ids.filter((id) => !this.panels.get(id)?.pointer)
    if (!ids.length) return
    this.store.update((c) => {
      for (const p of c.panels) {
        if (ids.includes(p.id)) {
          p.bounds = { ...p.bounds, x: p.bounds.x + move.dx, y: p.bounds.y + move.dy }
        }
      }
    })
    for (const id of ids) this.slideWindow(id)
  }

  /** Where a panel's window belongs now: mid-collapse it still holds the expanded size. */
  private targetBounds(rt: PanelRuntime, p: PanelConfig): Bounds {
    return rt.collapseTimer ? p.bounds : this.windowBounds(p)
  }

  /**
   * Slides a panel's window to where its config now puts it, alongside the
   * collapse animation. Moving a window is cheap, unlike the per-frame resize
   * the collapse animation avoids.
   */
  private slideWindow(panelId: string): void {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt || !p || rt.win.isDestroyed()) return
    this.stopSlide(panelId, false)
    const to = this.targetBounds(rt, p)
    const [fromX, fromY] = rt.win.getPosition()
    // At fractional display scaling, setPosition can grow the window by a
    // pixel each call, so every step sets the whole bounds, sized from the
    // config rather than from the window.
    const place = (x: number, y: number): void => rt.win.setBounds({ ...to, x, y })
    if (systemPreferences.getAnimationSettings().prefersReducedMotion) {
      place(to.x, to.y)
      return
    }
    const start = Date.now()
    const tick = (): void => {
      rt.slide = null
      if (rt.win.isDestroyed()) return
      const t = Math.min(1, (Date.now() - start) / COLLAPSE_ANIMATION_MS)
      // Close to the renderer's ease-out curve, so stacked panels move together.
      const k = 1 - (1 - t) ** 4
      place(Math.round(fromX + (to.x - fromX) * k), Math.round(fromY + (to.y - fromY) * k))
      if (t < 1) rt.slide = setTimeout(tick, 16)
    }
    rt.slide = setTimeout(tick, 16)
  }

  /** Stops a slide, optionally jumping to where it was headed. */
  private stopSlide(panelId: string, finish: boolean): void {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt?.slide) return
    clearTimeout(rt.slide)
    rt.slide = null
    if (finish && p && !rt.win.isDestroyed()) rt.win.setBounds(this.targetBounds(rt, p))
  }

  /** Shrinks a collapsing panel's window to its bar now, if it isn't already. */
  private finishCollapse(panelId: string): void {
    const rt = this.panels.get(panelId)
    if (!rt?.collapseTimer) return
    clearTimeout(rt.collapseTimer)
    rt.collapseTimer = null
    const p = this.store.getPanel(panelId)
    if (!p?.collapsed || rt.win.isDestroyed()) return
    this.applyBounds(rt.win, p)
    // The window can be deeper than the bar, and panels stacked against the
    // bar reach into that transparent part; keep it below them so it doesn't
    // take their clicks.
    sendToBottom(rt.win)
  }

  setLocked(locked: boolean): void {
    this.store.update((c) => (c.locked = locked))
    this.pushStateAll()
  }

  isHidden(): boolean {
    return this.hidden
  }

  setHidden(hidden: boolean): void {
    this.hidden = hidden
    for (const rt of this.panels.values()) {
      if (hidden) rt.win.hide()
      else {
        rt.win.showInactive()
        attachToDesktop(rt.win)
      }
    }
  }

  // --------------------------------------------------------- move and resize

  pointerStart(panelId: string, kind: 'move' | 'resize', edge?: ResizeEdge): void {
    const rt = this.panels.get(panelId)
    if (!rt || this.store.get().locked) return
    const p = this.store.getPanel(panelId)
    if (!p) return
    // A collapsed folder card has a fixed size.
    if (kind === 'resize' && p.collapsed && collapsedLength(p.headerShape) !== null) return
    // Moving mid-collapse would save the still-expanded window as the bar, and
    // mid-slide the window isn't yet where the config says.
    this.finishCollapse(panelId)
    this.stopSlide(panelId, true)
    const { panels, snapping } = this.store.get()
    // The size comes from the config: the window's own can be a pixel off at
    // fractional scaling, and reusing it would let panels creep bigger.
    const at = rt.win.getBounds()
    const startBounds = { ...this.windowBounds(p), x: at.x, y: at.y }
    rt.pointer = {
      kind,
      edge,
      startCursor: screen.getCursorScreenPoint(),
      startBounds,
      last: startBounds,
      startVisual: visualBounds(p),
      lines: snapping ? snapLines(panels.filter((o) => o.id !== panelId).map(visualBounds)) : null
    }
  }

  /** The panels' snap lines plus the edges of the work area under the cursor. */
  private withWorkArea(lines: SnapLines, cursor: { x: number; y: number }): SnapLines {
    const area = snapLines([screen.getDisplayNearestPoint(cursor).workArea])
    return { x: [...lines.x, ...area.x], y: [...lines.y, ...area.y] }
  }

  pointerMove(panelId: string): void {
    const rt = this.panels.get(panelId)
    const s = rt?.pointer
    if (!rt || !s) return
    const c = screen.getCursorScreenPoint()
    const dx = c.x - s.startCursor.x
    const dy = c.y - s.startCursor.y
    const lines = s.lines && this.withWorkArea(s.lines, c)
    let next: Bounds
    if (s.kind === 'move') {
      // Snap what the panel covers, then move the window by as much.
      const moved = { ...s.startVisual, x: s.startVisual.x + dx, y: s.startVisual.y + dy }
      const snapped = lines ? snapMove(moved, lines) : moved
      next = {
        ...s.startBounds,
        x: s.startBounds.x + dx + snapped.x - moved.x,
        y: s.startBounds.y + dy + snapped.y - moved.y
      }
    } else {
      // A collapsed panel only resizes along its title bar.
      const p = this.store.getPanel(panelId)
      const sideways = !!p?.collapsed && isSideways(p.collapseDirection)
      const flat = !!p?.collapsed && !sideways
      next = resizeBounds(s.startBounds, s.edge!, sideways ? 0 : dx, flat ? 0 : dy)
      if (flat) next = { ...next, y: s.startBounds.y, height: s.startBounds.height }
      if (sideways) next = { ...next, x: s.startBounds.x, width: s.startBounds.width }
      // Along the bar, the window's edges are the bar's, so they can snap as they are.
      const edge = flat
        ? s.edge!.replace(/[ns]/g, '')
        : sideways
          ? s.edge!.replace(/[ew]/g, '')
          : s.edge!
      if (lines) next = snapResize(next, edge, lines)
    }
    s.last = next
    rt.win.setBounds(next)
  }

  pointerEnd(panelId: string): void {
    const rt = this.panels.get(panelId)
    const s = rt?.pointer
    if (!rt || !s) return
    rt.pointer = null
    const b = s.last
    this.store.updatePanel(panelId, (p) => {
      if (s.kind === 'move') {
        const dx = b.x - s.startBounds.x
        const dy = b.y - s.startBounds.y
        p.bounds = { ...p.bounds, x: p.bounds.x + dx, y: p.bounds.y + dy }
        return
      }
      p.bounds = p.collapsed
        ? expandedFromCollapsed(
            b,
            p.bounds,
            p.collapseDirection,
            collapsedLength(p.headerShape),
            p.collapseAlign
          )
        : b
    })
  }

  // -------------------------------------------------------------------- items

  async listItems(panelId: string) {
    const p = this.store.getPanel(panelId)
    if (!p) return []
    const items = await listItems(p.folderPath, p.sortBy, p.order, p.cells)
    if (p.sortBy === 'custom') {
      // Remember the order new items arrived in and forget removed ones, so
      // later arrivals queue up behind them instead of sorting by name.
      const names = items.map((i) => i.fileName)
      const order = syncOrder(p.order, names)
      const cells = pruneCells(p.cells, names)
      if (order || cells) {
        this.store.updatePanel(panelId, (d) => {
          if (order) d.order = order
          if (cells) d.cells = cells
        })
      }
    }
    return items
  }

  private async pushItems(panelId: string): Promise<void> {
    const rt = this.panels.get(panelId)
    if (!rt || rt.win.isDestroyed()) return
    try {
      rt.win.webContents.send(IPC.itemsChanged, await this.listItems(panelId))
    } catch (err) {
      console.error('[panels] listing failed', err)
    }
  }

  /** Icon size in device pixels for a panel (icon size setting × display scale). */
  private iconPixels(p: PanelConfig): number {
    return Math.round(ICON_PIXELS[p.iconSize] * screen.getPrimaryDisplay().scaleFactor)
  }

  async getIcon(panelId: string, path: string): Promise<string | null> {
    const p = this.store.getPanel(panelId)
    if (!p || !this.isManagedPath(path)) return null
    return getIconDataUrl(path, this.iconPixels(p))
  }

  async getShortcutOverlay(panelId: string): Promise<string | null> {
    const p = this.store.getPanel(panelId)
    return p ? getShortcutOverlayDataUrl(this.iconPixels(p)) : null
  }

  /** Paths inside any panel's folder are ones the renderer may act on. */
  isManagedPath(path: string): boolean {
    return this.store
      .get()
      .panels.some((p) => isInside(path, p.folderPath) && !samePath(path, p.folderPath))
  }

  async openItem(path: string): Promise<void> {
    if (!this.isManagedPath(path)) return
    const err = await shell.openPath(path)
    if (err) dialog.showErrorBox('Could not open item', err)
  }

  /**
   * Files dropped from Explorer, the desktop or another panel: copy or move
   * them (see AppConfig.addMode) into the panel folder, into the cell `at` or
   * after the last item. Items from another panel always move.
   */
  async dropFiles(panelId: string, paths: string[], at: DropTarget | null = null): Promise<void> {
    const p = this.store.getPanel(panelId)
    if (!p) return
    const movable = paths.filter(
      (x) =>
        existsSync(x) &&
        dirname(x) !== x && // drive roots
        !samePath(x, this.opts.desktopDir) &&
        !this.store.get().panels.some((o) => samePath(x, o.folderPath))
    )
    const copy = this.store.get().addMode === 'copy'
    const moved = await this.moveAll(movable, p.folderPath, (src) =>
      copy && !this.isManagedPath(src) ? 'copy' : 'move'
    )
    // Without a spot to go to, items that were already here stay where they are.
    const placed = at ? moved : moved.filter((m) => !movable.some((src) => samePath(src, m)))
    if (!placed.length) return
    const names = placed.map((m) => basename(m))
    let cells: PlacedItem[] | null = null
    if (at && p.sortBy === 'custom') {
      // Lay the grid out as the panel shows it, then put the new items in.
      const shown = (await listItems(p.folderPath, 'custom', p.order, p.cells))
        .map((i) => i.fileName)
        .filter((n) => !names.includes(n))
      cells = placeAt(layoutGrid(shown, p.cells, at.cols), names, at.cell, at.cols).slice(
        0,
        MAX_PANEL_ITEMS
      )
    }
    this.store.updatePanel(panelId, (d) => {
      d.order = placeInOrder(d.order, names, null)
      if (cells) d.cells = cells
    })
    if (cells) this.pushState(panelId)
    // Items already in this folder don't change it, so nothing else refreshes.
    await this.pushItems(panelId)
  }

  /** Saves where the user put every item; arranging switches to custom order. */
  async arrangeItems(panelId: string, cells: PlacedItem[]): Promise<void> {
    if (!this.store.getPanel(panelId)) return
    const seen = new Set<string>()
    const unique = cells.filter((p) => {
      const k = p.name.toLowerCase()
      return !seen.has(k) && !!seen.add(k)
    })
    this.store.updatePanel(panelId, (d) => {
      d.cells = unique
      d.sortBy = 'custom'
    })
    this.pushState(panelId)
    await this.pushItems(panelId)
  }

  async moveItemTo(path: string, target: { panelId: string } | 'desktop'): Promise<void> {
    if (!this.isManagedPath(path)) return
    const dest =
      target === 'desktop' ? this.opts.desktopDir : this.store.getPanel(target.panelId)?.folderPath
    if (dest) await this.moveAll([path], dest)
  }

  /** Moves (or copies, where `how` says so) items; returns their new paths. */
  private async moveAll(
    paths: string[],
    destDir: string,
    how: (src: string) => 'copy' | 'move' = () => 'move'
  ): Promise<string[]> {
    const errors: string[] = []
    const moved: string[] = []
    let anyMove = false
    const toDesktop = samePath(destDir, this.opts.desktopDir)
    for (const src of paths) {
      try {
        const copy = how(src) === 'copy'
        anyMove ||= !copy
        const dest = await (copy ? copyInto : moveInto)(src, destDir)
        moved.push(dest)
        // Don't let the catching panel take back what the user sent to the desktop.
        if (toDesktop) this.catcher?.ignore(basename(dest))
      } catch (err) {
        errors.push(`${src}\n  ${(err as Error).message}`)
      }
    }
    if (errors.length) {
      dialog.showErrorBox(
        anyMove ? 'Some items could not be moved' : 'Some items could not be copied',
        errors.join('\n\n') +
          (anyMove
            ? '\n\nItems on the shared Public Desktop need administrator rights to move.'
            : '')
      )
    }
    return moved
  }

  async renameItem(path: string, newName: string): Promise<void> {
    if (!this.isManagedPath(path)) return
    try {
      const dest = await renameItem(path, newName)
      // Keep the renamed item where the user arranged it.
      const p = this.store.get().panels.find((x) => samePath(x.folderPath, dirname(path)))
      if (p) {
        this.store.updatePanel(p.id, (d) => {
          d.order = renameInOrder(d.order, basename(path), basename(dest))
          d.cells = renameInCells(d.cells, basename(path), basename(dest))
        })
      }
    } catch (err) {
      dialog.showErrorBox('Could not rename item', (err as Error).message)
    }
  }

  startDrag(panelId: string, path: string): void {
    const rt = this.panels.get(panelId)
    if (!rt || !this.isManagedPath(path)) return
    // A drop on the desktop copies the item there; that copy isn't new.
    this.catcher?.ignore(basename(path), 120_000)
    void getIconDataUrl(path, 32).then((url) => {
      if (rt.win.isDestroyed()) return
      // startDrag requires a non-empty icon.
      const icon = url ? nativeImage.createFromDataURL(url) : createTrayIcon()
      rt.win.webContents.startDrag({ file: path, icon })
    })
  }

  // -------------------------------------------------------------------- menus

  showItemMenu(panelId: string, path: string): void {
    const rt = this.panels.get(panelId)
    if (!rt || !this.isManagedPath(path)) return
    const others = this.store.get().panels.filter((p) => p.id !== panelId)
    const template: MenuItemConstructorOptions[] = [
      { label: 'Open', click: () => void this.openItem(path) },
      { label: 'Open file location', click: () => shell.showItemInFolder(path) },
      {
        label: 'Rename',
        click: () => rt.win.webContents.send(IPC.beginItemRename, path)
      },
      { type: 'separator' },
      {
        label: 'Move to',
        submenu: [
          { label: 'Desktop', click: () => void this.moveItemTo(path, 'desktop') },
          ...(others.length ? [{ type: 'separator' } as MenuItemConstructorOptions] : []),
          ...others.map((o) => ({
            label: o.title || '(untitled)',
            click: () => void this.moveItemTo(path, { panelId: o.id })
          }))
        ]
      },
      { type: 'separator' },
      {
        label: 'Move to Recycle Bin',
        click: () =>
          void shell
            .trashItem(path)
            .catch((e: Error) => dialog.showErrorBox('Could not delete item', e.message))
      }
    ]
    Menu.buildFromTemplate(template).popup({ window: rt.win })
  }

  private viewMenu(panelId: string): MenuItemConstructorOptions[] {
    const p = this.store.getPanel(panelId)!
    const sizes: [IconSize, string][] = [
      ['small', 'Small icons'],
      ['medium', 'Medium icons'],
      ['large', 'Large icons']
    ]
    const sorts: [SortBy, string][] = [
      ['custom', 'Custom (drag to arrange)'],
      ['name', 'Name'],
      ['type', 'Type'],
      ['modified', 'Date modified']
    ]
    return [
      {
        label: 'View',
        submenu: sizes.map(([v, label]) => ({
          label,
          type: 'radio',
          checked: p.iconSize === v,
          click: () => this.updatePanel(panelId, { iconSize: v })
        }))
      },
      {
        label: 'Sort by',
        submenu: sorts.map(([v, label]) => ({
          label,
          type: 'radio',
          checked: p.sortBy === v,
          click: () => this.updatePanel(panelId, { sortBy: v })
        }))
      }
    ]
  }

  showBackgroundMenu(panelId: string): void {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt || !p) return
    const template: MenuItemConstructorOptions[] = [
      ...this.viewMenu(panelId),
      { type: 'separator' },
      { label: 'Open folder in Explorer', click: () => void shell.openPath(p.folderPath) },
      { label: 'Panel settings…', click: () => void this.openSettings(panelId) },
      { type: 'separator' },
      { label: 'New panel', click: () => void this.newPanel() }
    ]
    Menu.buildFromTemplate(template).popup({ window: rt.win })
  }

  showTitleMenu(panelId: string): void {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt || !p) return
    const template: MenuItemConstructorOptions[] = [
      { label: 'Rename', click: () => rt.win.webContents.send(IPC.beginTitleEdit) },
      { label: p.collapsed ? 'Expand' : 'Collapse', click: () => this.toggleCollapsed(panelId) },
      { label: 'Panel settings…', click: () => void this.openSettings(panelId) },
      ...this.viewMenu(panelId),
      { type: 'separator' },
      { label: 'New panel', click: () => void this.newPanel() },
      {
        label: 'Lock all panels',
        type: 'checkbox',
        checked: this.store.get().locked,
        click: (item) => this.setLocked(item.checked)
      },
      { label: 'App settings…', click: () => void this.openAppSettings() },
      { type: 'separator' },
      { label: 'Delete panel…', click: () => void this.deletePanel(panelId) }
    ]
    Menu.buildFromTemplate(template).popup({ window: rt.win })
  }

  // --------------------------------------------------- create, delete, folder

  private uniqueFolder(title: string): string {
    const base = folderNameFromTitle(title)
    const used = (dir: string): boolean =>
      existsSync(dir) || this.store.get().panels.some((p) => samePath(p.folderPath, dir))
    let dir = join(this.opts.panelsRootDir, base)
    for (let n = 2; used(dir); n++) dir = join(this.opts.panelsRootDir, `${base} (${n})`)
    return dir
  }

  private addPanel(title: string): PanelConfig {
    const area = screen.getPrimaryDisplay().workArea
    const n = this.store.get().panels.length
    const offset = (n % 10) * 32
    const panel = createPanel({
      title,
      folderPath: this.uniqueFolder(title),
      bounds: {
        x: area.x + 40 + offset,
        y: area.y + 40 + offset,
        width: 360,
        height: 240
      }
    })
    this.store.update((c) => void c.panels.push(panel))
    return panel
  }

  async newPanel(): Promise<void> {
    const n = this.store.get().panels.length + 1
    const panel = this.addPanel(n === 1 ? 'Apps' : `Panel ${n}`)
    await this.openPanel(panel)
  }

  async deletePanel(panelId: string): Promise<void> {
    const p = this.store.getPanel(panelId)
    const rt = this.panels.get(panelId)
    if (!p || !rt) return
    const items = await listItems(p.folderPath, 'name')
    let moveBack = false
    if (items.length === 0) {
      const { response } = await dialog.showMessageBox(rt.settingsWin ?? rt.win, {
        type: 'question',
        title: 'Delete panel',
        message: `Delete the empty panel "${p.title}"?`,
        buttons: ['Delete', 'Cancel'],
        defaultId: 0,
        cancelId: 1
      })
      if (response === 1) return
    } else {
      const { response } = await dialog.showMessageBox(rt.settingsWin ?? rt.win, {
        type: 'question',
        title: 'Delete panel',
        message: `Delete the panel "${p.title}"?`,
        detail: `It contains ${items.length} item(s) stored in:\n${p.folderPath}`,
        buttons: ['Move items to Desktop and delete', 'Delete panel, keep folder', 'Cancel'],
        defaultId: 0,
        cancelId: 2
      })
      if (response === 2) return
      moveBack = response === 0
    }
    if (moveBack)
      await this.moveAll(
        items.map((i) => i.path),
        this.opts.desktopDir
      )
    // Only removes the folder when it is empty; never deletes user files.
    await fsp.rmdir(p.folderPath).catch(() => undefined)

    this.store.update((c) => (c.panels = c.panels.filter((x) => x.id !== panelId)))
    this.panels.delete(panelId)
    rt.settingsWin?.destroy()
    rt.win.destroy()
    this.syncCatcher()
  }

  async chooseFolder(panelId: string): Promise<void> {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt || !p) return
    const parent = rt.settingsWin ?? rt.win
    const res = await dialog.showOpenDialog(parent, {
      title: 'Choose the folder this panel shows',
      defaultPath: p.folderPath,
      properties: ['openDirectory', 'createDirectory']
    })
    const dir = res.filePaths[0]
    if (res.canceled || !dir) return
    if (samePath(dir, this.opts.desktopDir)) {
      dialog.showErrorBox('Choose another folder', 'A panel cannot show the Desktop folder itself.')
      return
    }
    this.store.updatePanel(panelId, (d) => (d.folderPath = dir))
    rt.watcher.watch(dir)
    this.pushState(panelId)
    await this.pushItems(panelId)
  }

  async openSettings(panelId: string): Promise<void> {
    const rt = this.panels.get(panelId)
    const p = this.store.getPanel(panelId)
    if (!rt || !p) return
    if (rt.settingsWin && !rt.settingsWin.isDestroyed()) {
      rt.settingsWin.show()
      rt.settingsWin.focus()
      return
    }
    const panelBounds = rt.win.getBounds()
    const area = screen.getDisplayMatching(panelBounds).workArea
    const win = new BrowserWindow({
      // Content size, so the frame doesn't eat into it. Short enough to sit
      // beside a panel on a small screen; the rest scrolls.
      width: 360,
      height: 520,
      useContentSize: true,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      autoHideMenuBar: true,
      icon: this.windowIcon(),
      title: `Panel settings: ${p.title}`,
      show: false,
      webPreferences: this.webPreferences()
    })
    win.setMenu(null)
    // Next to the panel, with the whole window (frame included) on screen.
    const { width, height } = win.getBounds()
    win.setPosition(
      Math.max(
        area.x,
        Math.min(panelBounds.x + panelBounds.width + 12, area.x + area.width - width)
      ),
      Math.max(area.y, Math.min(panelBounds.y, area.y + area.height - height))
    )
    rt.settingsWin = win
    const wcId = win.webContents.id
    this.senders.set(wcId, panelId)
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    win.webContents.on('will-navigate', (e) => e.preventDefault())
    win.once('ready-to-show', () => win.show())
    win.on('closed', () => {
      this.senders.delete(wcId)
      if (rt.settingsWin === win) rt.settingsWin = null
    })
    reloadOnCrash(win)
    await this.opts.loadRenderer(win, { view: 'settings', panel: panelId })
  }

  // ------------------------------------------------------------ app settings

  getAppSettings(): AppSettings {
    const { startWithWindows, locked, snapping, stacking, addMode } = this.store.get()
    return { startWithWindows, locked, snapping, stacking, addMode }
  }

  updateAppSettings(patch: Partial<AppSettings>): void {
    if (patch.startWithWindows !== undefined && canStartWithOs()) {
      setStartWithOs(patch.startWithWindows)
    }
    const before = this.store.get()
    const after = this.store.update((c) => Object.assign(c, patch))
    // Panels and their settings windows show these.
    if (
      after.locked !== before.locked ||
      after.stacking !== before.stacking ||
      after.addMode !== before.addMode
    ) {
      this.pushStateAll()
    }
  }

  isAppSettingsSender(webContentsId: number): boolean {
    return this.appWin?.webContentsId === webContentsId
  }

  async openAppSettings(): Promise<void> {
    if (this.appWin) {
      this.appWin.win.show()
      this.appWin.win.focus()
      return
    }
    const win = new BrowserWindow({
      // Content size, tall enough for every row without scrolling.
      width: 360,
      height: 550,
      useContentSize: true,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      autoHideMenuBar: true,
      icon: this.windowIcon(),
      title: 'Desktop Icons Sorter',
      show: false,
      webPreferences: this.webPreferences()
    })
    win.setMenu(null)
    this.appWin = { win, webContentsId: win.webContents.id }
    // Listen only while the window is open.
    const off = this.store.onChange(() => {
      if (!win.isDestroyed()) win.webContents.send(IPC.appSettingsChanged, this.getAppSettings())
    })
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    win.webContents.on('will-navigate', (e) => e.preventDefault())
    win.once('ready-to-show', () => win.show())
    win.on('closed', () => {
      off()
      if (this.appWin?.win === win) this.appWin = null
    })
    reloadOnCrash(win)
    await this.opts.loadRenderer(win, { view: 'app' })
  }
}
