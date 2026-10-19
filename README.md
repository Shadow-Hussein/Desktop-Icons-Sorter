# Desktop Icons Sorter

Rectangular, customizable panels on the Windows desktop for sorting your icons. It works like Stardock Fences: drop shortcuts into a panel to group them, then collapse the panel when you want a clean desktop.

![Panels on the Windows desktop: an Apps panel with a standard title bar, a Documents panel with a pill title bar, a Photos panel with a soft title bar, a Dev panel collapsed to a tile, and a Tools panel with a minimal title bar collapsed to a dock](docs/screenshots/desktop.webp)

Free and open source under the [MIT License](LICENSE).

## Features

- **Panels that live on the desktop.** They sit behind normal windows, stay out of the taskbar and Alt+Tab, and stay visible on "Show desktop" (Win+D).
- **Folder-backed.** Each panel shows a real folder (`%USERPROFILE%\DesktopSorter\<panel>` by default). Dropping a desktop icon onto a panel copies it into that folder, or moves it there so it leaves the cluttered desktop: choose Copy (the default) or Move in app settings. Icons dragged from one panel to another always move. The grouping survives restarts and Windows rearranging your icons.
- **Customizable per panel**, in a settings window with one-click controls (picture tiles for the title bar shape and style, arrow buttons for the collapse direction, toggle switches) that follows the Windows light or dark theme:
  - title (double-click it to rename)
  - background color and transparency
  - title bar color, transparency and text color
  - title bar shape: default, dock (a collapsed panel shows a row of its apps you can click to open) or tile (a collapsed panel is a square with four of its apps, at the start, middle or end of its edge)
  - title bar style, with any shape: standard, pill (a rounded capsule), minimal (just the title, no bar, expanded or collapsed; the collapse arrow shows when you point at the panel, and a collapsed panel always shows its expand arrow) or soft (the bar fades out at its ends and blends into the panel instead of hard edges)
  - color presets
  - icon size (small, medium, large) and sort order (custom, name, type, date modified)
- **Arrange icons your way.** Drag an icon to any spot in its panel, including empty ones, and it stays there, gaps and all. Drop it on another icon and that icon and the ones right after it slide along to the next gap, like a phone's home screen. Custom order is the default; new items go after the last icon, and an item dropped in from elsewhere lands where you drop it. If a panel gets too narrow for an icon's column, the icon moves to the next free spot until the panel is wide enough again. Picking Name, Type or Date sorts the panel instead, and dragging an icon switches back to custom order.
- **Catch new desktop items.** Turn on "Catch new desktop items" in a panel's settings and new shortcuts, downloads and files that appear on your desktop are copied (or moved, as set in app settings) into that panel. One panel catches at a time. It waits for a download to finish, and leaves alone renamed desktop items, empty new files and folders (probably still being named), and items you move to the desktop from a panel. It watches your own Desktop folder only, so shortcuts an installer puts on the shared Public Desktop (for all users) aren't caught.
- **Collapsible.** Click the chevron or double-click an empty part of the title bar to roll the panel up to its title bar. In panel settings, choose the edge it collapses toward: up, down, left or right. The title bar sits on that edge, so a bottom bar suits panels near the taskbar and a side bar suits panels along a screen edge.
- **Stackable.** Put panels against each other, say one under another, and collapsing the top one moves the ones below it up to fill the space; expanding it moves them back down. It works the same way for panels that collapse down, left or right. Turn it off for every panel in app settings, or leave one panel out of it in its panel settings.
- **Move and resize** by dragging the title bar or the edges. While you drag, a panel's edges snap to screen edges and to other panels' edges, so panels line up and sit flush against each other; keep dragging to pull away. Turn snapping off in app settings, and lock all panels from the tray.
- **App settings** (tray menu > Settings…, or App settings… in a panel's title bar menu): whether panels copy or move the items added to them, snapping, stacking, locking panels and starting with Windows.
- **Starts with Windows** after a fresh install (turn it off in app settings or the tray menu). The portable exe doesn't, since it isn't installed anywhere.
- **Item actions:**
  - double-click to open
  - drag out to Explorer, the desktop or another panel
  - right-click for Open file location, Rename (or F2), Move to Desktop or another panel, and Move to Recycle Bin
- **Tray menu:** New panel, Show/Hide panels, Lock panels, Start with Windows, Settings…, Open config folder, Quit.
- **Safe deletes.** Deleting a panel asks whether to move its items back to the Desktop. Files are never deleted.

<p align="center">
  <img src="docs/screenshots/settings.webp" width="640" alt="The panel settings window, with picture tiles for the title bar shape and style, and the app settings window, next to an Apps panel, a Photos panel and a Dev panel collapsed to a tile">
</p>

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

The app and installer icon is `build/icon.ico`. It's generated by `build/make-icon.py` (Python + Pillow); edit that script and run `python3 build/make-icon.py` to change it.

## Releasing

1. Bump `version` in `package.json` (for example with `npm version 0.2.0 --no-git-tag-version`) and push it to `main`.
2. Tag that commit and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

The **Release** workflow then builds both executables on Windows and publishes them as a GitHub release. The release notes list every commit subject since the previous tag (version-bump commits are left out), so write commit subjects that make sense to users. Preview them with `node scripts/release-notes.mjs v0.2.0` after tagging locally. It fails if the tag doesn't match the `package.json` version. The executables aren't code-signed, so Windows SmartScreen will warn on first run.

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
    store.ts               Validated, atomic, debounced config.json
    geometry.ts            Resize, snapping to screen and panel edges, keeping panels on screen
    stack.ts               Which panels move when a stacked panel collapses or expands
    icons.ts, watcher.ts   Shell icons, folder watching (fs.watch)
    ipc.ts                 IPC handlers; every payload is validated
    validate.ts            Small schema checks for config and IPC payloads
  preload/index.ts         contextBridge API (renderers are sandboxed)
  renderer/                React UI: panel view, panel settings and app settings windows
tests/                     Unit tests
```

### How the desktop layer works

Each panel window is made an _owned_ window of `Progman` (the desktop shell window) and kept at the bottom of the z-order. Owned windows always stay above their owner, so panels sit directly on the desktop, below other apps, and stay visible on Win+D. Unlike rendering into the wallpaper `WorkerW` layer, panels still get mouse input. If Explorer restarts, the app re-attaches the panels automatically.

## Notes and limitations

- Items on the shared _Public Desktop_ (`C:\Users\Public\Desktop`) need administrator rights to move. The app tells you when a move fails.
- Renaming a panel does not rename its folder. Use **Panel settings → Folder → Change…** to point a panel at a different folder.

## License

MIT — see [LICENSE](LICENSE).
