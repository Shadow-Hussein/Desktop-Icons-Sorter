# CLAUDE.md

Desktop Icons Sorter is a Windows tray app (Rust, Win32 via the `windows` crate, Direct2D) that puts rectangular, folder-backed panels on the desktop for grouping icons, like Stardock Fences. It was an Electron app until version 0.2.0; the config file format is unchanged. See [README.md](README.md) for the feature list, project layout and how the desktop layer works.

## Git branches

- Work directly on `main`. Commit to `main`, and don't create feature branches.
- Use a different branch only when explicitly told to work in that branch.

## Optimization first

The app runs all day in the background, so memory use is the top priority. Every added feature or change must keep memory use as low as possible.

- Measure before and after any change that could affect memory: run the release build with a throwaway profile (`DESKTOP_SORTER_USER_DATA`, `DESKTOP_SORTER_PANELS_DIR`) and read its private memory. See [Measuring memory](#measuring-memory).
- Keep it one process on one UI thread, and keep drawing in software (Direct2D `D2D1_RENDER_TARGET_TYPE_SOFTWARE` into layered windows). Don't add a feature that needs a GPU device, a browser engine or a thread per panel.
- Prefer the standard library and `windows` crate features over new crates. Add a crate only if a few lines of code can't do the job, and check what it costs in memory and exe size. Runtime dependencies today: `windows`, `windows-core`, `windows-numerics`, `serde`, `serde_json`.
- Bound every cache and replace stale entries instead of adding new ones next to them. `src/win/icons.rs` is the pattern: at most 300 icons and 6 MB of pixels, least recently used out first, a changed file replaces its entry.
- Release resources when they're not shown: hiding panels drops the icon cache and the drawing surface, settings windows are destroyed on close, the desktop watch only runs while a panel catches new items.
- Don't poll. Waits are events: folder changes come from `ReadDirectoryChangesW` handles waited on in the main loop, Explorer restarts from the `TaskbarCreated` broadcast. Timers only run while something is happening (an animation, a debounced save, a catch waiting for a download to settle).
- Draw only on change, never in a loop.

### Measuring memory

In PowerShell, from the repo root:

```powershell
$env:DESKTOP_SORTER_USER_DATA = "$env:TEMP\ds-measure\profile"
$env:DESKTOP_SORTER_PANELS_DIR = "$env:TEMP\ds-measure\panels"
cargo build --release
Start-Process .\target\release\desktop-icons-sorter.exe
# wait ~10 s for panels to settle, then:
$p = Get-Process desktop-icons-sorter
"{0:N1} MB private" -f ($p.PrivateMemorySize64 / 1MB)
$p | Stop-Process -Force
```

- Measure the release build, not `cargo run` (debug builds are much larger).
- Compare with the same number of panels each time (past measurements use four, each with six shortcuts), and take a few runs: ±1 MB between runs is noise. With four panels the app uses about 16 MB.
- The first run creates one "Apps" panel; add more from the tray to reach four, then restart and measure. Or write a `config.json` with four panels into the profile folder first.
- Put the before/after numbers in the commit message body.
- Delete the temp folder afterwards (wait a moment after `Stop-Process`, or Windows reports files still in use).

## Commands

| Command                                     | What it does                                    |
| ------------------------------------------- | ----------------------------------------------- |
| `cargo run`                                 | Debug build with a console for log output       |
| `cargo build --release`                     | Release exe in `target\release`                 |
| `cargo test`                                | Unit tests                                      |
| `cargo test grid`                           | Tests whose names match `grid`                  |
| `cargo clippy --all-targets -- -D warnings` | Lints                                           |
| `cargo fmt`                                 | Format (`cargo fmt --check` to only check)      |

The installer is built with NSIS from `installer\installer.nsi`; the command is in the comment at the top of the script.

Before calling a change done, run what CI runs: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`. All three must pass.

## Architecture

- **Pure logic** (`src/lib.rs` and the modules it declares, crate name `sorter`): config, geometry, stacking, the icon grid, folder operations, the desktop catcher's decisions, panel layout and hit testing. None of it imports `windows`, so it's all unit tested with `cargo test`.
- **The app** (`src/main.rs` and the rest of `src/`): `main.rs` takes the single-instance mutex, starts OLE and runs the message loop (`MsgWaitForMultipleObjectsEx` over window messages and folder-watch events). `app.rs` holds the `App`: config store, panels, settings windows, tray, catcher. `procs.rs` has the window procedures (input, menus, drag and drop), `panel.rs` draws a panel and does the move/resize math, `settings.rs` is the panel and app settings windows.
- **Win32** (`src/win/`): desktop layer, DPI and monitors, Direct2D/DirectWrite drawing, shell icons, folder watching, tray, menus, dialogs, the rename box, shell operations. Keep `unsafe` Win32 calls in here or right next to the message handling that needs them.
- **State**: the `App` lives in a thread-local and window procedures reach it with `with_app`. A message sent while a handler is running (by `SetWindowPos`, `SetCapture`, `CreateWindowEx`…) finds the app busy and gets default handling.
- **Modal UI** (menus, dialogs, `DoDragDrop`, `ShellExecuteEx`) runs its own message loop, which calls back into the window procedures. Never run it inside `with_app`: queue an `app::Deferred` and let `run_deferred` run it after the handler returns.
- **Config** lives in `config.json` under `%APPDATA%\desktop-icons-sorter` (the folder the Electron app used). `ConfigStore` writes it atomically (temp file, then rename), 300 ms after the last change (`App::changed`), and again on quit. A file that fails validation is renamed to `config.bad.json` and the valid panels are kept.
- **Panel contents** are a real folder on disk. Moving an item into a panel moves the file; nothing is ever deleted (only moved to the Recycle Bin when the user asks).
- **Coordinates**: panel bounds in the config are DIPs, as Electron saved them. `win/dpi.rs` converts with the scale of the monitor a rectangle is on.

## Conventions

### Adding a panel setting

1. Add the field to `PanelConfig` in `src/model.rs`.
2. Parse it in `parse_panel` in `src/config.rs` with `optional(…, default, …)`, so existing config files still load.
3. Add the control to `panel_page` in `src/settings.rs` and its `Act`, and handle it in `App::act`. If it changes the window (size, position), check `App::paint` and `panel::window_bounds`.
4. In the tests in `src/config.rs`, check the default and that an invalid value drops the panel.
5. Update the feature list in `README.md`.

### Code style

- `rustfmt` with `rustfmt.toml` (100 columns), clippy clean with `-D warnings`. LF line endings.
- Comments explain why, not what, and are short. Document non-obvious Windows behavior next to the code that works around it.
- Keep pure logic (geometry, validation, folder operations, parsing) in the library modules, not in `src/win/` or the window procedures, so it can be unit tested. Tests sit next to the code in `#[cfg(test)] mod tests`.
- Add or update tests for changes to pure logic.

### Commit messages

- Release notes are built from commit subjects (`scripts/release-notes.mjs`), so write the subject for users: imperative, plain words, no prefix like `feat:` (for example "Let the user choose which way a panel collapses").
- In the body, say what changed and why, and include before/after memory numbers when the change could affect memory.
- Update `README.md` in the same commit when user-visible behavior changes.

## Gotchas

- Panels are layered windows updated with `UpdateLayeredWindow`, which can't show child controls. Text editing on a panel uses a separate popup (`win/edit.rs`) owned by the panel.
- Fully transparent pixels of a layered window don't get clicks, so a panel's background is drawn with at least a little alpha even at 0% opacity.
- Panels force themselves to the bottom of the z-order in `WM_WINDOWPOSCHANGING`; anything that should appear above them (rename box, menus, dialogs) must be owned by them or be a normal top-level window.
- Drags out of a panel offer only copy and link (a drag never takes an item out of its panel), so drop handlers must accept `copy` as well as `move`.
- Items on the Public Desktop (`C:\Users\Public\Desktop`) need admin rights to move; surface the error instead of failing silently.
- A `config.json` saved by PowerShell or Notepad can start with a byte order mark; the loader strips it.
- Testing with injected mouse input (`SendInput`, `mouse_event`) silently fails while an elevated window has the foreground. Post messages to the app's windows instead, or check `GetLastInputInfo`.
- `target/`, `dist/` and `graphify-out/` are generated and gitignored. Don't edit or commit them.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:

- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
