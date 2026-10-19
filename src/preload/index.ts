import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron'
import {
  IPC,
  type AppSettings,
  type PanelItem,
  type PanelState,
  type SorterApi
} from '../shared/types'

const params = new URLSearchParams(window.location.search)
const view = params.get('view')

function subscribe<T extends unknown[]>(channel: string, cb: (...args: T) => void): () => void {
  const listener = (_e: IpcRendererEvent, ...args: unknown[]): void => cb(...(args as T))
  ipcRenderer.on(channel, listener)
  return () => ipcRenderer.removeListener(channel, listener)
}

const api: SorterApi = {
  panelId: params.get('panel') ?? '',
  view: view === 'settings' || view === 'app' ? view : 'panel',

  getState: () => ipcRenderer.invoke(IPC.getState),
  onStateChanged: (cb) => subscribe<[PanelState]>(IPC.stateChanged, cb),
  updatePanel: (patch) => ipcRenderer.invoke(IPC.updatePanel, patch),
  toggleCollapsed: () => ipcRenderer.invoke(IPC.toggleCollapsed),

  moveStart: () => ipcRenderer.send(IPC.moveStart),
  resizeStart: (edge) => ipcRenderer.send(IPC.resizeStart, edge),
  pointerMove: () => ipcRenderer.send(IPC.pointerMove),
  pointerEnd: () => ipcRenderer.send(IPC.pointerEnd),

  showTitleMenu: () => ipcRenderer.send(IPC.titleMenu),
  onBeginTitleEdit: (cb) => subscribe<[]>(IPC.beginTitleEdit, cb),

  listItems: () => ipcRenderer.invoke(IPC.listItems),
  onItemsChanged: (cb) => subscribe<[PanelItem[]]>(IPC.itemsChanged, cb),
  getIcon: (path) => ipcRenderer.invoke(IPC.getIcon, path),
  getShortcutOverlay: () => ipcRenderer.invoke(IPC.getShortcutOverlay),
  openItem: (path) => ipcRenderer.invoke(IPC.openItem, path),
  dropFiles: (paths, at) => ipcRenderer.invoke(IPC.dropFiles, paths, at ?? null),
  arrangeItems: (cells) => ipcRenderer.invoke(IPC.arrangeItems, cells),
  renameItem: (path, newName) => ipcRenderer.invoke(IPC.renameItem, path, newName),
  showItemMenu: (path) => ipcRenderer.send(IPC.itemMenu, path),
  showBackgroundMenu: () => ipcRenderer.send(IPC.backgroundMenu),
  onBeginItemRename: (cb) => subscribe<[string]>(IPC.beginItemRename, cb),
  startDrag: (path) => ipcRenderer.send(IPC.startDrag, path),
  getPathForFile: (file) => webUtils.getPathForFile(file),

  openSettings: () => ipcRenderer.send(IPC.openSettings),
  chooseFolder: () => ipcRenderer.invoke(IPC.settingsChooseFolder),
  deletePanel: () => ipcRenderer.invoke(IPC.settingsDeletePanel),

  getAppSettings: () => ipcRenderer.invoke(IPC.getAppSettings),
  onAppSettingsChanged: (cb) => subscribe<[AppSettings]>(IPC.appSettingsChanged, cb),
  updateAppSettings: (patch) => ipcRenderer.invoke(IPC.updateAppSettings, patch)
}

contextBridge.exposeInMainWorld('sorter', api)
