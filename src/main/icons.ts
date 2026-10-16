import { app, nativeImage, shell, type NativeImage } from 'electron'
import { promises as fsp } from 'node:fs'
import { resolveIconSource, type IconSourceDeps } from './iconSource'
import { extractIcon } from './winIcons'

/**
 * Recently used icons by path and size. Map order doubles as LRU order, and a
 * changed file replaces its entry, so stale icons never pile up.
 */
const cache = new Map<string, { mtime: number; url: string }>()
const MAX_CACHE = 300

const iconSourceDeps: IconSourceDeps = {
  readShortcutLink: (path) => shell.readShortcutLink(path),
  readFile: (path) => fsp.readFile(path, 'utf8'),
  exists: (path) =>
    fsp.access(path).then(
      () => true,
      () => false
    ),
  env: process.env
}

function usable(img: NativeImage | null | undefined): img is NativeImage {
  return !!img && !img.isEmpty()
}

/**
 * Windows: find the icon Explorer would show (a shortcut's icon location or
 * target, a .url's IconFile, an .exe's own icon) and extract it at the exact
 * size. Returns null when there's nothing better than the generic icon.
 */
async function windowsShellIcon(path: string, pixels: number): Promise<NativeImage | null> {
  const source = await resolveIconSource(path, iconSourceDeps)
  if (!source) return null
  const raw = extractIcon(source.file, source.index, pixels)
  if (raw) return nativeImage.createFromBitmap(raw.data, { width: raw.width, height: raw.height })
  // The target holds no icon resources (a folder or a document): its own
  // shell icon still beats the generic shortcut icon.
  if (source.file !== path) {
    const img = await app.getFileIcon(source.file, { size: 'large' }).catch(() => null)
    if (usable(img)) return img
  }
  return null
}

/**
 * Returns a PNG data URL for the file's icon at roughly `pixels` size.
 *
 * `app.getFileIcon` alone isn't enough on Windows: Chromium looks icons up by
 * extension for everything but .exe/.dll/.ico, so every .lnk shows the generic
 * "shortcut to a file" page and every .url the internet-shortcut globe, and it
 * tops out at 32px.
 */
export async function getIconDataUrl(path: string, pixels: number): Promise<string | null> {
  let mtime: number
  try {
    mtime = (await fsp.stat(path)).mtimeMs
  } catch {
    return null
  }
  const key = `${path}|${pixels}`
  const hit = cache.get(key)
  if (hit) {
    cache.delete(key)
    if (hit.mtime === mtime) {
      cache.set(key, hit)
      return hit.url
    }
  }

  let img: NativeImage | null = null
  if (process.platform === 'win32') {
    img = await windowsShellIcon(path, pixels).catch(() => null)
  }
  if (!usable(img) && (process.platform === 'win32' || process.platform === 'darwin')) {
    // Real thumbnails for images, videos, documents with previews…
    img = await nativeImage
      .createThumbnailFromPath(path, { width: pixels, height: pixels })
      .catch(() => null)
  }
  if (!usable(img)) {
    img = await app.getFileIcon(path, { size: pixels > 32 ? 'large' : 'normal' }).catch(() => null)
  }

  const url = usable(img) ? img.toDataURL() : null
  if (url) {
    cache.set(key, { mtime, url })
    if (cache.size > MAX_CACHE) cache.delete(cache.keys().next().value as string)
  }
  return url
}
