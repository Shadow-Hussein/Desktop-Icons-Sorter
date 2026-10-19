import { ipcMain, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import { IPC } from '../shared/types'
import type { PanelManager } from './panelManager'
import { appSettingsPatchSchema, cellsSchema, dropTargetSchema, panelPatchSchema } from './store'
import { array, oneOf, string } from './validate'

const pathArg = string({ min: 1, max: 32_767 })
const pathsArg = array(pathArg, { max: 1000 })
const nameArg = string({ min: 1, max: 255 })
const edgeArg = oneOf(['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'])

/**
 * Registers all IPC handlers. Every handler resolves the panel from the
 * sending window, so a renderer can only ever act on its own panel, and every
 * payload is validated before it reaches the panel manager.
 */
export function registerIpc(pm: PanelManager): void {
  const panelOf = (e: IpcMainEvent | IpcMainInvokeEvent): string => {
    const id = pm.panelIdForSender(e.sender.id)
    if (!id) throw new Error('IPC from an unknown window')
    return id
  }
  const handle = (channel: string, fn: (panelId: string, ...args: unknown[]) => unknown): void => {
    ipcMain.handle(channel, (e, ...args) => fn(panelOf(e), ...args))
  }
  const on = (channel: string, fn: (panelId: string, ...args: unknown[]) => void): void => {
    ipcMain.on(channel, (e, ...args) => {
      const id = pm.panelIdForSender(e.sender.id)
      if (id) fn(id, ...args)
    })
  }

  handle(IPC.getState, (id) => pm.getState(id))
  handle(IPC.updatePanel, (id, patch) => pm.updatePanel(id, panelPatchSchema.parse(patch)))
  handle(IPC.toggleCollapsed, (id) => pm.toggleCollapsed(id))

  on(IPC.moveStart, (id) => pm.pointerStart(id, 'move'))
  on(IPC.resizeStart, (id, edge) => pm.pointerStart(id, 'resize', edgeArg.parse(edge)))
  on(IPC.pointerMove, (id) => pm.pointerMove(id))
  on(IPC.pointerEnd, (id) => pm.pointerEnd(id))
  on(IPC.titleMenu, (id) => pm.showTitleMenu(id))

  handle(IPC.listItems, (id) => pm.listItems(id))
  handle(IPC.getIcon, (id, path) => pm.getIcon(id, pathArg.parse(path)))
  handle(IPC.getShortcutOverlay, (id) => pm.getShortcutOverlay(id))
  handle(IPC.openItem, (_id, path) => pm.openItem(pathArg.parse(path)))
  handle(IPC.dropFiles, (id, paths, at) =>
    pm.dropFiles(id, pathsArg.parse(paths), at == null ? null : dropTargetSchema.parse(at))
  )
  handle(IPC.arrangeItems, (id, cells) => pm.arrangeItems(id, cellsSchema.parse(cells)))
  handle(IPC.renameItem, (_id, path, name) =>
    pm.renameItem(pathArg.parse(path), nameArg.parse(name))
  )
  on(IPC.itemMenu, (id, path) => pm.showItemMenu(id, pathArg.parse(path)))
  on(IPC.backgroundMenu, (id) => pm.showBackgroundMenu(id))
  on(IPC.startDrag, (id, path) => pm.startDrag(id, pathArg.parse(path)))

  on(IPC.openSettings, (id) => void pm.openSettings(id))
  handle(IPC.settingsChooseFolder, (id) => pm.chooseFolder(id))
  handle(IPC.settingsDeletePanel, (id) => pm.deletePanel(id))

  // The app settings window belongs to no panel, so it is checked on its own.
  const fromAppSettings = (e: IpcMainInvokeEvent): void => {
    if (!pm.isAppSettingsSender(e.sender.id)) throw new Error('IPC from an unknown window')
  }
  ipcMain.handle(IPC.getAppSettings, (e) => {
    fromAppSettings(e)
    return pm.getAppSettings()
  })
  ipcMain.handle(IPC.updateAppSettings, (e, patch) => {
    fromAppSettings(e)
    pm.updateAppSettings(appSettingsPatchSchema.parse(patch))
  })
}
