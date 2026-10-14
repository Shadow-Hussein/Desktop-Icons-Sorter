import chokidar, { type FSWatcher } from 'chokidar'

/** Watches one folder (non-recursively) and calls `onChange` debounced. */
export class FolderWatcher {
  private watcher: FSWatcher | null = null
  private timer: NodeJS.Timeout | null = null

  constructor(
    private readonly onChange: () => void,
    private readonly debounceMs = 150
  ) {}

  watch(folder: string): void {
    void this.close()
    this.watcher = chokidar.watch(folder, {
      depth: 0,
      ignoreInitial: true,
      awaitWriteFinish: { stabilityThreshold: 200, pollInterval: 50 }
    })
    this.watcher.on('all', () => this.schedule())
    this.watcher.on('error', () => this.schedule())
  }

  private schedule(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      this.onChange()
    }, this.debounceMs)
  }

  async close(): Promise<void> {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    const w = this.watcher
    this.watcher = null
    await w?.close()
  }
}
