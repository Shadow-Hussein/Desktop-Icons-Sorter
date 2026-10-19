import { existsSync, promises as fsp, readdirSync, watch, type FSWatcher } from 'node:fs'
import { extname, join } from 'node:path'

/** Partial downloads and temp files; the real item shows up under its own name when done. */
const TEMP_EXTENSIONS = new Set([
  '.crdownload',
  '.part',
  '.partial',
  '.download',
  '.opdownload',
  '.tmp',
  '.temp'
])
const IGNORED_NAMES = new Set(['desktop.ini', 'thumbs.db'])

/** How long after an item stops changing it counts as finished (downloads, copies). */
const SETTLE_MS = 3000
/** A name that appears this soon after another vanished is that item being renamed. */
const RENAME_WINDOW_MS = 1500
/** How often a move that failed (a file still locked, say) is tried again. */
const MAX_ATTEMPTS = 3
/** Bounds the pending checks if something floods the desktop with files. */
const MAX_PENDING = 200

const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin'
const key = (name: string): string => (caseInsensitive ? name.toLowerCase() : name)

/** Whether a new desktop item with this name may be moved into a panel. */
export function isCatchable(name: string): boolean {
  const lower = name.toLowerCase()
  return (
    !IGNORED_NAMES.has(lower) &&
    !lower.startsWith('~$') &&
    !TEMP_EXTENSIONS.has(extname(lower)) &&
    !/[\\/]/.test(name)
  )
}

async function isEmpty(path: string): Promise<boolean> {
  const st = await fsp.stat(path)
  return st.isDirectory() ? (await fsp.readdir(path)).length === 0 : st.size === 0
}

/**
 * Watches the Desktop folder (non-recursively, with fs.watch) and hands each
 * new item to `onItem` once it has stopped changing. Renames of items already
 * there are not new items. Empty new files and folders are left alone: they
 * were most likely just made from the desktop's New menu and are being named.
 */
export class DesktopCatcher {
  private watcher: FSWatcher | null = null
  /** Names on the desktop, to tell renames from new items. */
  private known = new Set<string>()
  private lastVanished = 0
  /** Items being moved into a panel; their vanishing is not half of a rename. */
  private readonly movingOut = new Set<string>()
  private readonly pending = new Map<string, NodeJS.Timeout>()
  /** Names the app itself puts on the desktop → when to stop ignoring them. */
  private readonly ignored = new Map<string, number>()

  constructor(
    private readonly dir: string,
    /** Copies or moves the item into a panel; false when it failed and is worth another try. */
    private readonly onItem: (path: string) => Promise<boolean>,
    private readonly settleMs = SETTLE_MS
  ) {}

  start(): void {
    if (this.watcher) return
    try {
      this.known = new Set(readdirSync(this.dir).map(key))
      this.watcher = watch(this.dir, (event, name) => name && this.saw(event, name.toString()))
    } catch {
      return
    }
    this.watcher.on('error', () => this.stop())
  }

  stop(): void {
    this.watcher?.close()
    this.watcher = null
    for (const t of this.pending.values()) clearTimeout(t)
    this.pending.clear()
    this.known.clear()
    this.movingOut.clear()
    this.ignored.clear()
  }

  /** Leaves an item the app moves onto the desktop itself alone for a while. */
  ignore(name: string, ms = 30_000): void {
    const now = Date.now()
    for (const [k, until] of this.ignored) if (until <= now) this.ignored.delete(k)
    this.ignored.set(key(name), now + ms)
  }

  private saw(event: string, name: string): void {
    const k = key(name)
    if (event === 'rename') {
      const exists = existsSync(join(this.dir, name))
      if (!exists) {
        // A finished download (.crdownload renamed to its real name) is new,
        // and so is anything that appears while we move an item out.
        const renameSource = this.known.delete(k) && isCatchable(name) && !this.movingOut.delete(k)
        if (renameSource) this.lastVanished = Date.now()
        return
      }
      if (this.known.has(k)) return
      this.known.add(k)
      if (Date.now() - this.lastVanished < RENAME_WINDOW_MS) return
    } else if (!this.pending.has(k)) {
      // Writes to items that aren't new.
      return
    }
    if (!isCatchable(name)) return
    this.schedule(name, 1)
  }

  private schedule(name: string, attempt: number): void {
    const k = key(name)
    const prev = this.pending.get(k)
    if (prev) clearTimeout(prev)
    else if (this.pending.size >= MAX_PENDING) return
    const delay = this.settleMs * attempt
    this.pending.set(
      k,
      setTimeout(() => {
        this.pending.delete(k)
        void this.check(name, attempt)
      }, delay)
    )
  }

  private async check(name: string, attempt: number): Promise<void> {
    if ((this.ignored.get(key(name)) ?? 0) > Date.now()) return
    const path = join(this.dir, name)
    try {
      if (await isEmpty(path)) return
    } catch {
      return // Gone already.
    }
    if (!this.watcher) return
    this.movingOut.add(key(name))
    const handled = await this.onItem(path)
    // Copied, or failed: the item stays, so it won't vanish and clear this.
    if (!handled || existsSync(path)) this.movingOut.delete(key(name))
    if (!handled && attempt < MAX_ATTEMPTS && this.watcher) this.schedule(name, attempt + 1)
  }
}
