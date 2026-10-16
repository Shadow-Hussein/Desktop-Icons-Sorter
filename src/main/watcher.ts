import { watch, type FSWatcher } from 'node:fs'

/**
 * Watches one folder (non-recursively) and calls `onChange` debounced.
 * Uses Node's own fs.watch (ReadDirectoryChangesW on Windows), which keeps no
 * per-file state, unlike chokidar.
 */
export class FolderWatcher {
  private watcher: FSWatcher | null = null
  private timer: NodeJS.Timeout | null = null

  constructor(
    private readonly onChange: () => void,
    private readonly debounceMs = 150
  ) {}

  watch(folder: string): void {
    this.close()
    try {
      this.watcher = watch(folder, () => this.schedule())
    } catch {
      // Missing or unreadable folder: the panel shows whatever listing finds.
      return
    }
    // The folder was deleted or became inaccessible: refresh, stop watching.
    this.watcher.on('error', () => {
      this.closeWatcher()
      this.schedule()
    })
  }

  private schedule(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      this.onChange()
    }, this.debounceMs)
  }

  private closeWatcher(): void {
    this.watcher?.close()
    this.watcher = null
  }

  close(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.closeWatcher()
  }
}
