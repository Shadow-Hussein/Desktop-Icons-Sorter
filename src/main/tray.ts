import { Menu, Tray, app, shell } from 'electron'
import { dirname } from 'node:path'
import { canStartWithOs, type PanelManager } from './panelManager'
import { createTrayIcon } from './trayIcon'

export function createTray(pm: PanelManager): Tray {
  const tray = new Tray(createTrayIcon())
  tray.setToolTip('Desktop Icons Sorter')

  const rebuild = (): void => {
    const config = pm.store.get()
    tray.setContextMenu(
      Menu.buildFromTemplate([
        { label: 'New panel', click: () => void pm.newPanel() },
        {
          label: pm.isHidden() ? 'Show panels' : 'Hide panels',
          click: () => {
            pm.setHidden(!pm.isHidden())
            rebuild()
          }
        },
        {
          label: 'Lock panels',
          type: 'checkbox',
          checked: config.locked,
          click: (item) => pm.setLocked(item.checked)
        },
        { type: 'separator' },
        {
          label: 'Start with Windows',
          type: 'checkbox',
          checked: config.startWithWindows,
          enabled: canStartWithOs(),
          click: (item) => pm.updateAppSettings({ startWithWindows: item.checked })
        },
        { label: 'Settings…', click: () => void pm.openAppSettings() },
        {
          label: 'Open config folder',
          click: () => void shell.openPath(dirname(pm.store.filePath))
        },
        { type: 'separator' },
        { label: 'Quit', click: () => app.quit() }
      ])
    )
  }

  rebuild()
  pm.store.onChange(rebuild)
  tray.on('click', () => {
    pm.setHidden(!pm.isHidden())
    rebuild()
  })
  return tray
}
