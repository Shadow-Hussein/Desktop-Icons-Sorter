import {
  BrowserWindow,
  Menu,
  dialog,
  nativeImage,
  screen,
  shell,
  type MenuItemConstructorOptions
} from 'electron'
import { existsSync, promises as fsp } from 'node:fs'
import { dirname, join } from 'node:path'
import type {
  Bounds,
  IconSize,
  PanelConfig,
  PanelPatch,
  PanelState,
  ResizeEdge,
  SortBy
} from '../shared/types'
import { ICON_PIXELS, IPC, TITLE_BAR_HEIGHT } from '../shared/types'
import { attachToDesktop, needsReattach, sendToBottom } from './desktopLayer'
import { isInside, listItems, moveInto, renameItem, samePath } from './folderService'
import { clampToWorkAreas, resizeBounds, snapToArea } from './geometry'
import { getIconDataUrl } from './icons'
import { getShortcutOverlayDataUrl } from './shortcutOverlay'
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
}

interface PanelRuntime {
  win: BrowserWindow
  watcher: FolderWatcher
  pointer: PointerSession | null
  settingsWin: BrowserWindow | null
}

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

export class PanelManager {
  private readonly panels = new Map<string, PanelRuntime>()
  /** webContents id → panel id, for both panel and settings windows. */
  private readonly senders = new Map<number, string>()
  private hidden = false
  private reattachTimer: NodeJS.Timeout | null = null

  constructor(private readonly opts: PanelManagerOptions) {}

  get store(): ConfigStore {
    return this.opts.store
  }

  // ---------------------------------------------------------------- lifecycle

  async start(): Promise<void> {
    if (this.store.get().panels.length === 0) this.addPanel('Apps')
    for (const p of this.store.get().panels) await this.openPanel(p)

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
  }

  panelIdForSender(webContentsId: number): string | undefined {
    return this.senders.get(webContentsId)
  }

  private windowBounds(p: PanelConfig): Bounds {
    return p.collapsed ? { ...p.bounds, height: TITLE_BAR_HEIGHT } : p.bounds
  }

  private async openPanel(panel: PanelConfig): Promise<void> {
    const workAreas = screen.getAllDisplays().map((d) => d.workArea)
    const bounds = clampToWorkAreas(panel.bounds, workAreas)
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
      webPreferences: {
        preload: this.opts.preloadPath,
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        spellcheck: false
      }
    })
    const rt: PanelRuntime = {
      win,
      watcher: new FolderWatcher(() => void this.pushItems(panel.id)),
      pointer: null,
      settingsWin: null
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
    win.on('blur', () => sendToBottom(win))
    win.on('closed', () => {
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
      const b = clampToWorkAreas(p.bounds, workAreas)
      if (b !== p.bounds) {
        const updated = this.store.updatePanel(p.id, (d) => (d.bounds = b))
        if (updated) this.panels.get(p.id)?.win.setBounds(this.windowBounds(updated))
      }
    }
  }

  // ------------------------------------------------------------ panel config

  getState(panelId: string): PanelState | undefined {
    const panel = this.store.getPanel(panelId)
    return panel && { panel, locked: this.store.get().locked }
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
    if (patch.sortBy !== undefined && patch.sortBy !== before.sortBy) void this.pushItems(panelId)
  }

  toggleCollapsed(panelId: string): void {
    const updated = this.store.updatePanel(panelId, (p) => (p.collapsed = !p.collapsed))
    const rt = this.panels.get(panelId)
    if (!updated || !rt) return
    // Let the renderer switch layout first so the content never looks squashed.
    this.pushState(panelId)
    this.animateHeight(rt.win, this.windowBounds(updated).height)
  }

  private animateHeight(win: BrowserWindow, target: number): void {
    const start = win.getBounds()
    const steps = 6
    let i = 0
    const tick = (): void => {
      if (win.isDestroyed()) return
      i++
      const h = Math.round(start.height + ((target - start.height) * i) / steps)
      win.setBounds({ ...start, height: i >= steps ? target : h })
      if (i < steps) setTimeout(tick, 12)
    }
    tick()
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
    rt.pointer = {
      kind,
      edge,
      startCursor: screen.getCursorScreenPoint(),
      startBounds: rt.win.getBounds()
    }
  }

  pointerMove(panelId: string): void {
    const rt = this.panels.get(panelId)
    const s = rt?.pointer
    if (!rt || !s) return
    const c = screen.getCursorScreenPoint()
    const dx = c.x - s.startCursor.x
    const dy = c.y - s.startCursor.y
    let next: Bounds
    if (s.kind === 'move') {
      next = { ...s.startBounds, x: s.startBounds.x + dx, y: s.startBounds.y + dy }
      next = snapToArea(next, screen.getDisplayNearestPoint(c).workArea)
    } else {
      const collapsed = this.store.getPanel(panelId)?.collapsed
      next = resizeBounds(s.startBounds, s.edge!, dx, collapsed ? 0 : dy)
      if (collapsed) next = { ...next, y: s.startBounds.y, height: s.startBounds.height }
    }
    rt.win.setBounds(next)
  }

  pointerEnd(panelId: string): void {
    const rt = this.panels.get(panelId)
    if (!rt?.pointer) return
    rt.pointer = null
    const b = rt.win.getBounds()
    this.store.updatePanel(panelId, (p) => {
      p.bounds = p.collapsed ? { ...p.bounds, x: b.x, y: b.y, width: b.width } : b
    })
  }

  // -------------------------------------------------------------------- items

  async listItems(panelId: string) {
    const p = this.store.getPanel(panelId)
    return p ? listItems(p.folderPath, p.sortBy) : []
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

  /** Files dropped from Explorer or the desktop: move them into the panel folder. */
  async dropFiles(panelId: string, paths: string[]): Promise<void> {
    const p = this.store.getPanel(panelId)
    if (!p) return
    const movable = paths.filter(
      (x) =>
        existsSync(x) &&
        dirname(x) !== x && // drive roots
        !samePath(x, this.opts.desktopDir) &&
        !this.store.get().panels.some((o) => samePath(x, o.folderPath))
    )
    await this.moveAll(movable, p.folderPath)
  }

  async moveItemTo(path: string, target: { panelId: string } | 'desktop'): Promise<void> {
    if (!this.isManagedPath(path)) return
    const dest =
      target === 'desktop' ? this.opts.desktopDir : this.store.getPanel(target.panelId)?.folderPath
    if (dest) await this.moveAll([path], dest)
  }

  private async moveAll(paths: string[], destDir: string): Promise<void> {
    const errors: string[] = []
    for (const src of paths) {
      try {
        await moveInto(src, destDir)
      } catch (err) {
        errors.push(`${src}\n  ${(err as Error).message}`)
      }
    }
    if (errors.length) {
      dialog.showErrorBox(
        'Some items could not be moved',
        errors.join('\n\n') +
          '\n\nItems on the shared Public Desktop need administrator rights to move.'
      )
    }
  }

  async renameItem(path: string, newName: string): Promise<void> {
    if (!this.isManagedPath(path)) return
    try {
      await renameItem(path, newName)
    } catch (err) {
      dialog.showErrorBox('Could not rename item', (err as Error).message)
    }
  }

  startDrag(panelId: string, path: string): void {
    const rt = this.panels.get(panelId)
    if (!rt || !this.isManagedPath(path)) return
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
      // Content size, so the frame doesn't eat into it; tall enough to show
      // every section (678 px) without scrolling.
      width: 360,
      height: 690,
      useContentSize: true,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      autoHideMenuBar: true,
      title: `Panel settings: ${p.title}`,
      show: false,
      webPreferences: {
        preload: this.opts.preloadPath,
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
        spellcheck: false
      }
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
}
