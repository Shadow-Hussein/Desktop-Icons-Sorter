# Desktop Icons Sorter

Rectangular, customizable panels on the Windows desktop for sorting your icons. It works like Stardock Fences: drop shortcuts into a panel to group them, then collapse the panel when you want a clean desktop.

## Features

- **Panels that live on the desktop.** They sit behind normal windows, stay out of the taskbar and Alt+Tab, and stay visible on "Show desktop" (Win+D).
- **Folder-backed.** Each panel shows a real folder (`%USERPROFILE%\DesktopSorter\<panel>` by default). Dropping a desktop icon onto a panel moves the file into that folder, so it leaves the cluttered desktop. The grouping survives restarts and Windows rearranging your icons.
- **Customizable per panel:**
  - title (double-click it to rename)
  - background color and transparency
  - title bar color, transparency and text color
  - color presets
  - icon size (small, medium, large) and sort order (name, type, date modified)
- **Collapsible.** Click the chevron or double-click an empty part of the title bar to roll the panel up to its title bar.
- **Move and resize** by dragging the title bar or the edges. Panels snap to screen edges, and you can lock all panels from the tray.
- **Item actions:**
  - double-click to open
  - drag out to Explorer, the desktop or another panel
  - right-click for Open file location, Rename (or F2), Move to Desktop or another panel, and Move to Recycle Bin
- **Tray menu:** New panel, Show/Hide panels, Lock panels, Start with Windows, Open config folder, Quit.
- **Safe deletes.** Deleting a panel asks whether to move its items back to the Desktop. Files are never deleted.

## Getting started

Requirements: Windows 10 or 11 and Node.js 22 or later.

```bash
npm install
npm run dev        # run with hot reload
```

Build a Windows installer and a portable exe into `dist/`:

```bash
npm run dist:win
```

This produces `desktop-icons-sorter-<version>-setup.exe` (installer) and `desktop-icons-sorter-<version>-portable.exe`.

## Releasing

1. Bump `version` in `package.json` (for example with `npm version 0.2.0 --no-git-tag-version`) and merge it to `main`.
2. Tag that commit and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

The **Release** workflow then builds both executables on Windows and publishes them, with generated release notes, as a GitHub release. It fails if the tag doesn't match the `package.json` version. The executables aren't code-signed, so Windows SmartScreen will warn on first run.

## Development

| Command             | What it does                                  |
| ------------------- | --------------------------------------------- |
| `npm run dev`       | Start the app with hot reload                 |
| `npm run build`     | Build main, preload and renderer into `out/`  |
| `npm start`         | Run the built app                             |
| `npm test`          | Unit tests (vitest)                           |
| `npm run lint`      | ESLint                                        |
| `npm run typecheck` | TypeScript, for main/preload and the renderer |
| `npm run format`    | Prettier                                      |
| `npm run dist:dir`  | Unpacked Windows build (for quick testing)    |

These environment variables are useful for testing with a throwaway profile:

- `DESKTOP_SORTER_USER_DATA`: where `config.json` is stored
- `DESKTOP_SORTER_PANELS_DIR`: parent folder for new panels' folders

### Project layout

```
src/
  shared/types.ts          Config types, IPC channel names, the window.sorter API
  main/
    index.ts               App lifecycle, single-instance lock, tray
    panelManager.ts        One frameless transparent window per panel; menus, move/resize, collapse
    desktopLayer.ts        Win32 (via koffi): keeps panels on the desktop layer
    folderService.ts       List, move (collision-safe, cross-drive) and rename items
    store.ts               Validated (zod), atomic, debounced config.json
    geometry.ts            Resize, edge snapping, keeping panels on screen
    icons.ts, watcher.ts   Shell icons, folder watching (chokidar)
    ipc.ts                 IPC handlers; every payload is validated
  preload/index.ts         contextBridge API (renderers are sandboxed)
  renderer/                React UI: panel view and settings window
tests/                     Unit tests
```

### How the desktop layer works

Each panel window is made an _owned_ window of `Progman` (the desktop shell window) and kept at the bottom of the z-order. Owned windows always stay above their owner, so panels sit directly on the desktop, below other apps, and stay visible on Win+D. Unlike rendering into the wallpaper `WorkerW` layer, panels still get mouse input. If Explorer restarts, the app re-attaches the panels automatically.

## Notes and limitations

- Items on the shared _Public Desktop_ (`C:\Users\Public\Desktop`) need administrator rights to move. The app tells you when a move fails.
- Renaming a panel does not rename its folder. Use **Panel settings → Folder → Change…** to point a panel at a different folder.
