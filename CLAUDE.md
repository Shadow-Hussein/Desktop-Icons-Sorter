# CLAUDE.md

Desktop Icons Sorter is a Windows tray app (Electron + React + TypeScript) that puts rectangular, folder-backed panels on the desktop for grouping icons, like Stardock Fences. See [README.md](README.md) for the feature list, project layout and how the desktop layer works.

## Git branches

- Work directly on `main`. Commit to `main`, and don't create feature branches.
- Use a different branch only when explicitly told to work in that branch.

## Optimization first

The app runs all day in the background, so memory use is the top priority. Every added feature or change must keep memory use as low as possible.

- Measure before and after any change that could affect memory: run the built app with a throwaway profile (`DESKTOP_SORTER_USER_DATA`, `DESKTOP_SORTER_PANELS_DIR`) and sum the private memory of all its Electron processes. See [Measuring memory](#measuring-memory).
- Keep panels in one shared renderer process (`process-per-site`) and keep software rendering (`disableHardwareAcceleration`). Don't add a feature that needs a separate renderer process per panel or GPU acceleration.
- Prefer Node/Electron built-ins over new dependencies. Add a dependency only if a built-in or a few lines of code can't do the job, and check how much memory it costs to load. (zod was replaced by `src/main/validate.ts` and chokidar by `fs.watch` for this reason; don't bring them back.)
- Bound every cache (LRU with a small cap) and replace stale entries instead of adding new ones next to them. `src/main/icons.ts` is the pattern: a `Map` used in insertion order, capped at 300.
- Release resources when they're not shown: unmount hidden UI, close watchers, destroy windows that aren't needed.
- Don't use polling or timers when an event is available. The 10 s Explorer re-attach check in `PanelManager.start()` is the one existing poll; don't add more.
- Keep the renderer bundle small: every panel shares it, so a heavy UI library costs memory in every panel.

### Measuring memory

In PowerShell, from the repo root:

```powershell
$env:DESKTOP_SORTER_USER_DATA = "$env:TEMP\ds-measure\profile"
$env:DESKTOP_SORTER_PANELS_DIR = "$env:TEMP\ds-measure\panels"
npm run build
Start-Process .\node_modules\electron\dist\electron.exe -ArgumentList '.'
# wait ~10 s for panels to settle, then:
$p = Get-Process electron | Where-Object { $_.Path -like "$PWD\*" }
"{0} processes, {1:N1} MB private" -f $p.Count, (($p | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB)
$p | Stop-Process -Force
```

- Measure the built app, not `npm run dev` (the dev server and unminified bundle skew the numbers).
- Compare with the same number of panels each time (past measurements use four), and take a few runs: ±3 MB between runs is noise.
- The first run creates one "Apps" panel; add more from the tray to reach four, then restart and measure.
- Put the before/after numbers in the commit message body.
- Delete the temp folder afterwards (wait a moment after `Stop-Process`, or Windows reports files still in use).

## Commands

| Command                              | What it does                                  |
| ------------------------------------ | --------------------------------------------- |
| `npm run dev`                        | Run with hot reload                           |
| `npm run build`                      | Build main, preload and renderer into `out/`  |
| `npm start`                          | Build and run the app                         |
| `npm test`                           | Unit tests (vitest, Node environment)         |
| `npx vitest run tests/store.test.ts` | Run one test file                             |
| `npm run lint`                       | ESLint                                        |
| `npm run format:check`               | Prettier check (`npm run format` to fix)      |
| `npm run typecheck`                  | `tsc` for main/preload and for the renderer   |
| `npm run dist:dir`                   | Unpacked Windows build in `dist/win-unpacked` |
| `npm run dist:win`                   | Installer and portable exe in `dist/`         |

Before calling a change done, run what CI runs: `npm run lint`, `npm run format:check`, `npm run typecheck` and `npm test`. All four must pass.

## Architecture

- **Main process** (`src/main/`): `index.ts` sets the memory switches, the single-instance lock and the throwaway-profile env vars, then creates `ConfigStore`, `PanelManager`, IPC handlers and the tray. `PanelManager` owns every window: one frameless transparent window per panel, plus an optional settings window per panel.
- **Preload** (`src/preload/index.ts`): exposes `window.sorter` (the `SorterApi` in `src/shared/types.ts`) through `contextBridge`. The renderer learns which panel it is from the `panel` and `view` query parameters.
- **Renderer** (`src/renderer/`): one React bundle. `main.tsx` renders `<Panel />` or `<SettingsView />` depending on `view`. State comes from the main process through `usePanelState()`; the renderer never keeps its own copy of the config.
- **Shared** (`src/shared/types.ts`): config types, constants (sizes) and the `IPC` channel names. Both sides import from it; keep it free of Node and DOM code.
- **Config** lives in `config.json` under the user data folder. `ConfigStore` writes it debounced and atomically (temp file, then rename), and `flushSync()` runs on quit. A file that fails validation is renamed to `config.bad.json` and the valid panels are kept.
- **Panel contents** are a real folder on disk. Moving an item into a panel moves the file; nothing is ever deleted (only moved to the Recycle Bin when the user asks).
- **Win32** calls go through koffi in `desktopLayer.ts` and `winIcons.ts`. Every Win32 function must be a no-op off Windows so the app still runs elsewhere.

## Conventions

### Security

- Every window uses `sandbox: true`, `contextIsolation: true`, `nodeIntegration: false`, denies `window.open` and blocks navigation. Keep these on every new window.
- `registerIpc()` resolves the panel from the sending window, so a renderer can act only on its own panel. Never accept a panel id from the renderer.
- Validate every IPC payload with the schemas in `validate.ts`/`store.ts` before it reaches `PanelManager`.

### Adding an IPC call

1. Add the channel name to `IPC` in `src/shared/types.ts` and the method to `SorterApi`.
2. Implement it in `src/preload/index.ts`: `ipcRenderer.invoke` for calls that return a result, `ipcRenderer.send` for fire-and-forget, `subscribe()` for main → renderer events.
3. Register it in `src/main/ipc.ts` with `handle` or `on`, parsing every argument.
4. Implement the behavior as a `PanelManager` method that takes the panel id first.

### Adding a panel setting

Recent examples: commits `3b87159` (collapse direction) and `307e73c` (title bar shape).

1. Add the field to `PanelConfig` and, if the renderer may change it, to `PanelPatch` in `src/shared/types.ts`.
2. In `src/main/store.ts`, add it to `panelSchema` wrapped in `withDefault(...)` so existing config files still load, and to `panelPatchSchema` **without** a default (a default there would reset the field on every unrelated patch).
3. Add the control to `SettingsView.tsx` and handle it in the main process if it changes the window (size, position).
4. In `tests/store.test.ts`, check the default and that an invalid value is rejected.
5. Update the feature list in `README.md`.

### Code style

- Prettier: single quotes, no semicolons, 100-column lines, no trailing commas. 2-space indent, LF line endings.
- Strict TypeScript with `noUnusedLocals`/`noUnusedParameters`. Prefix intentionally unused parameters with `_`.
- Comments explain why, not what, and are short. Document non-obvious Windows or Electron behavior next to the code that works around it.
- Keep pure logic (geometry, validation, folder operations, parsing) in modules that don't import `electron`, so it can be unit tested. Where a module must import `electron`, mock it in the test (`vi.mock('electron', ...)`, see `tests/shortcutOverlay.test.ts`).
- Add or update tests in `tests/` for changes to pure logic.

### Commit messages

- Release notes are built from commit subjects (`scripts/release-notes.mjs`), so write the subject for users: imperative, plain words, no prefix like `feat:` (for example "Let the user choose which way a panel collapses").
- In the body, say what changed and why, and include before/after memory numbers when the change could affect memory.
- Update `README.md` in the same commit when user-visible behavior changes.

## Gotchas

- Windows won't make a transparent window shorter than about 38 px and rounds sizes at fractional scaling. `PanelManager.placeWindow()` works around this; use it instead of calling `setBounds` directly on panel windows.
- A panel joining the shared renderer process gets an opaque white background, so transparency is re-applied on every `did-finish-load`. Keep that when touching window setup.
- Because all windows share one renderer process, a renderer crash blanks every panel at once. `reloadOnCrash()` handles it; attach it to any new window.
- Drags started from a panel (`webContents.startDrag`) only allow copy or link, so drop handlers must accept `copy` as well as `move`.
- Items on the Public Desktop (`C:\Users\Public\Desktop`) need admin rights to move; surface the error instead of failing silently.
- `out/`, `dist/` and `graphify-out/` are generated and gitignored. Don't edit or commit them.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:

- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
