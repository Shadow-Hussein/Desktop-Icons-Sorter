import { app, type BrowserWindow } from 'electron'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { registerIpc } from './ipc'
import { PanelManager, setStartWithOs } from './panelManager'
import { ConfigStore } from './store'
import { createTray } from './tray'

if (process.platform === 'win32') app.setAppUserModelId('com.shadowhussein.desktopiconssorter')

// Memory: every panel loads the same local page, so they can share one
// renderer process instead of one each (~30 MB per panel). The UI is mostly
// static, so software rendering is plenty and the GPU process shrinks from
// ~95 MB to ~17 MB.
app.commandLine.appendSwitch('process-per-site')
app.disableHardwareAcceleration()

// Lets UI tests and side-by-side dev runs use a throwaway profile.
if (process.env.DESKTOP_SORTER_USER_DATA) {
  app.setPath('userData', process.env.DESKTOP_SORTER_USER_DATA)
}

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  let manager: PanelManager | null = null

  app.on('second-instance', () => manager?.setHidden(false))
  // Panels live in the tray; closing every window must not quit the app.
  app.on('window-all-closed', () => undefined)

  app.whenReady().then(async () => {
    const configPath = join(app.getPath('userData'), 'config.json')
    const firstRun = !existsSync(configPath)
    const store = new ConfigStore(configPath)
    store.load()
    // A fresh install starts with Windows. Not the portable exe, which isn't
    // installed anywhere, and not a dev run, which would add electron.exe.
    if (
      firstRun &&
      app.isPackaged &&
      process.platform === 'win32' &&
      !process.env.PORTABLE_EXECUTABLE_FILE
    ) {
      store.update((c) => (c.startWithWindows = true))
    }

    const loadRenderer = async (
      win: BrowserWindow,
      query: Record<string, string>
    ): Promise<void> => {
      const devUrl = process.env.ELECTRON_RENDERER_URL
      if (!app.isPackaged && devUrl) {
        await win.loadURL(`${devUrl}?${new URLSearchParams(query)}`)
      } else {
        await win.loadFile(join(__dirname, '../renderer/index.html'), { query })
      }
    }

    manager = new PanelManager({
      store,
      desktopDir: app.getPath('desktop'),
      panelsRootDir:
        process.env.DESKTOP_SORTER_PANELS_DIR ?? join(app.getPath('home'), 'DesktopSorter'),
      preloadPath: join(__dirname, '../preload/index.js'),
      loadRenderer
    })
    registerIpc(manager)
    createTray(manager)

    // Keep the OS login item in sync with the saved preference.
    if (process.platform === 'win32' && app.isPackaged) {
      setStartWithOs(store.get().startWithWindows)
    }

    await manager.start()

    app.on('before-quit', () => {
      manager?.stop()
      store.flushSync()
    })
  })
}
