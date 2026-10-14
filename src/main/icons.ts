import { app, nativeImage } from 'electron'
import { promises as fsp } from 'node:fs'

const cache = new Map<string, string>()
const MAX_CACHE = 2000

/**
 * Returns a PNG data URL for the file's shell icon at roughly `pixels` size.
 * On Windows, `createThumbnailFromPath` goes through the shell image factory
 * and returns crisp large icons (including shortcut targets); `getFileIcon`
 * is the fallback and tops out at 32px.
 */
export async function getIconDataUrl(path: string, pixels: number): Promise<string | null> {
  let mtime: number
  try {
    mtime = (await fsp.stat(path)).mtimeMs
  } catch {
    return null
  }
  const key = `${path}|${mtime}|${pixels}`
  const hit = cache.get(key)
  if (hit) return hit

  let url: string | null = null
  if (process.platform === 'win32' || process.platform === 'darwin') {
    try {
      const img = await nativeImage.createThumbnailFromPath(path, { width: pixels, height: pixels })
      if (!img.isEmpty()) url = img.toDataURL()
    } catch {
      // Fall through to getFileIcon.
    }
  }
  if (!url) {
    try {
      const img = await app.getFileIcon(path, { size: pixels > 32 ? 'large' : 'normal' })
      if (!img.isEmpty()) url = img.toDataURL()
    } catch {
      url = null
    }
  }
  if (url) {
    if (cache.size >= MAX_CACHE) cache.delete(cache.keys().next().value as string)
    cache.set(key, url)
  }
  return url
}
