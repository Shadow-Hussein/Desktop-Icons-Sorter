import { promises as fsp } from 'node:fs'
import { basename, dirname, extname, join, relative, resolve, isAbsolute } from 'node:path'
import type { PanelItem, SortBy } from '../shared/types'

/** Extensions Explorer hides on the desktop even with "show extensions" on. */
const HIDDEN_EXTENSIONS = new Set(['.lnk', '.url', '.pif'])
/** Files that should never show up as panel items. */
const IGNORED_NAMES = new Set(['desktop.ini', 'thumbs.db', '.ds_store'])

const caseInsensitive = process.platform === 'win32' || process.platform === 'darwin'

export function isShortcut(fileName: string): boolean {
  return HIDDEN_EXTENSIONS.has(extname(fileName).toLowerCase())
}

export function displayNameFor(fileName: string): string {
  const ext = extname(fileName)
  return HIDDEN_EXTENSIONS.has(ext.toLowerCase()) ? fileName.slice(0, -ext.length) : fileName
}

export function samePath(a: string, b: string): boolean {
  const ra = resolve(a)
  const rb = resolve(b)
  return caseInsensitive ? ra.toLowerCase() === rb.toLowerCase() : ra === rb
}

/** True when `child` is `parent` itself or somewhere below it. */
export function isInside(child: string, parent: string): boolean {
  const rel = relative(
    caseInsensitive ? resolve(parent).toLowerCase() : resolve(parent),
    caseInsensitive ? resolve(child).toLowerCase() : resolve(child)
  )
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel))
}

export function sortItems(items: PanelItem[], sortBy: SortBy): PanelItem[] {
  const byName = (a: PanelItem, b: PanelItem): number =>
    a.displayName.localeCompare(b.displayName, undefined, { numeric: true, sensitivity: 'base' })
  const sorted = [...items]
  switch (sortBy) {
    case 'type':
      sorted.sort((a, b) => {
        if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1
        const ea = extname(a.fileName).toLowerCase()
        const eb = extname(b.fileName).toLowerCase()
        return ea === eb ? byName(a, b) : ea.localeCompare(eb)
      })
      break
    case 'modified':
      sorted.sort((a, b) => b.modifiedMs - a.modifiedMs || byName(a, b))
      break
    default:
      sorted.sort(byName)
  }
  return sorted
}

export async function listItems(folder: string, sortBy: SortBy): Promise<PanelItem[]> {
  let entries
  try {
    entries = await fsp.readdir(folder, { withFileTypes: true })
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return []
    throw err
  }
  const items = await Promise.all(
    entries
      .filter((e) => !IGNORED_NAMES.has(e.name.toLowerCase()) && !e.name.startsWith('~$'))
      .map(async (e): Promise<PanelItem | null> => {
        const path = join(folder, e.name)
        try {
          const st = await fsp.stat(path)
          return {
            fileName: e.name,
            displayName: displayNameFor(e.name),
            path,
            isDirectory: st.isDirectory(),
            isShortcut: !st.isDirectory() && isShortcut(e.name),
            modifiedMs: st.mtimeMs
          }
        } catch {
          // Broken symlink or file removed while listing.
          return null
        }
      })
  )
  return sortItems(
    items.filter((i): i is PanelItem => i !== null),
    sortBy
  )
}

async function exists(path: string): Promise<boolean> {
  try {
    await fsp.lstat(path)
    return true
  } catch {
    return false
  }
}

/** Returns a path in `dir` for `fileName` that does not exist yet: "name (2).ext", … */
export async function uniqueDestination(dir: string, fileName: string): Promise<string> {
  let candidate = join(dir, fileName)
  if (!(await exists(candidate))) return candidate
  const ext = extname(fileName)
  const stem = fileName.slice(0, fileName.length - ext.length)
  for (let n = 2; n < 10_000; n++) {
    candidate = join(dir, `${stem} (${n})${ext}`)
    if (!(await exists(candidate))) return candidate
  }
  throw new Error(`Could not find a free name for ${fileName} in ${dir}`)
}

/**
 * Move a file or folder into `destDir`, renaming on collision. Falls back to
 * copy + delete when the source is on a different drive. Returns the new path.
 */
export async function moveInto(src: string, destDir: string): Promise<string> {
  if (samePath(dirname(src), destDir)) return src
  if (isInside(destDir, src)) throw new Error('Cannot move a folder into itself.')
  await fsp.mkdir(destDir, { recursive: true })
  const dest = await uniqueDestination(destDir, basename(src))
  try {
    await fsp.rename(src, dest)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'EXDEV') throw err
    await fsp.cp(src, dest, { recursive: true, errorOnExist: true, force: false })
    await fsp.rm(src, { recursive: true, force: true })
  }
  return dest
}

/** Characters Windows does not allow in file names. */
const INVALID_NAME = /[<>:"/\\|?*\x00-\x1f]/ // eslint-disable-line no-control-regex

/**
 * Rename an item in place. If the user typed the display name of a shortcut
 * (no .lnk), the hidden extension is kept. Returns the new path.
 */
export async function renameItem(path: string, newName: string): Promise<string> {
  const trimmed = newName.trim().replace(/[. ]+$/, '')
  if (!trimmed || INVALID_NAME.test(trimmed) || trimmed === '.' || trimmed === '..') {
    throw new Error(`"${newName}" is not a valid file name.`)
  }
  const oldExt = extname(path)
  let fileName = trimmed
  if (
    HIDDEN_EXTENSIONS.has(oldExt.toLowerCase()) &&
    extname(trimmed).toLowerCase() !== oldExt.toLowerCase()
  ) {
    fileName += oldExt
  }
  const dest = join(dirname(path), fileName)
  if (samePath(dest, path)) {
    // Case-only rename, which a plain rename handles fine.
    if (dest !== path) await fsp.rename(path, dest)
    return dest
  }
  if (await exists(dest)) throw new Error(`An item named "${fileName}" already exists.`)
  await fsp.rename(path, dest)
  return dest
}
