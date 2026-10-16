# CLAUDE.md

## Git branches

- Work directly on `main`. Commit to `main`, and don't create feature branches.
- Use a different branch only when explicitly told to work in that branch.

## Optimization first

The app runs all day in the background, so memory use is the top priority. Every added feature or change must keep memory use as low as possible.

- Measure before and after any change that could affect memory: run the built app with a throwaway profile (`DESKTOP_SORTER_USER_DATA`, `DESKTOP_SORTER_PANELS_DIR`) and sum the private memory of all its Electron processes.
- Keep panels in one shared renderer process (`process-per-site`) and keep software rendering (`disableHardwareAcceleration`). Don't add a feature that needs a separate renderer process per panel or GPU acceleration.
- Prefer Node/Electron built-ins over new dependencies. Add a dependency only if a built-in or a few lines of code can't do the job, and check how much memory it costs to load.
- Bound every cache (LRU with a small cap) and replace stale entries instead of adding new ones next to them.
- Release resources when they're not shown: unmount hidden UI, close watchers, destroy windows that aren't needed.
- Don't use polling or timers when an event is available.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:

- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
