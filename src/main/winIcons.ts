/**
 * Extracts an icon from a file's icon resources (.exe, .dll, .ico…) at an
 * exact pixel size using Win32 (via koffi). Electron's `app.getFileIcon`
 * picks icons by file extension for shortcuts and tops out at 32px, so it
 * can't show the real icon of a .lnk/.url target at 48–128px.
 *
 * Kept free of Electron imports so it can be tested with plain Node on Windows.
 */

export interface RawIcon {
  width: number
  height: number
  /** Top-down BGRA pixels with premultiplied alpha (what Electron's createFromBitmap expects). */
  data: Buffer
}

interface Win32 {
  PrivateExtractIconsW(
    file: string,
    index: number,
    cx: number,
    cy: number,
    icons: [number | bigint],
    ids: [number],
    count: number,
    flags: number
  ): number
  GetIconInfo(icon: number | bigint, info: Record<string, number | bigint>): boolean
  DestroyIcon(icon: number | bigint): boolean
  GetDC(hwnd: number): number | bigint
  ReleaseDC(hwnd: number, hdc: number | bigint): number
  GetDIBits(
    hdc: number | bigint,
    bitmap: number | bigint,
    start: number,
    lines: number,
    bits: Buffer,
    info: Buffer,
    usage: number
  ): number
  DeleteObject(obj: number | bigint): boolean
}

let win32: Win32 | null | undefined

function api(): Win32 | null {
  if (win32 !== undefined) return win32
  if (process.platform !== 'win32') return (win32 = null)
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const koffi = require('koffi') as typeof import('koffi')
    const user32 = koffi.load('user32.dll')
    const gdi32 = koffi.load('gdi32.dll')
    koffi.struct('ICONINFO', {
      fIcon: 'int',
      xHotspot: 'uint32_t',
      yHotspot: 'uint32_t',
      hbmMask: 'intptr_t',
      hbmColor: 'intptr_t'
    })
    win32 = {
      PrivateExtractIconsW: user32.func(
        'uint32_t __stdcall PrivateExtractIconsW(const char16_t *file, int index, int cx, int cy, _Out_ intptr_t *icons, _Out_ uint32_t *ids, uint32_t count, uint32_t flags)'
      ),
      GetIconInfo: user32.func('bool __stdcall GetIconInfo(intptr_t icon, _Out_ ICONINFO *info)'),
      DestroyIcon: user32.func('bool __stdcall DestroyIcon(intptr_t icon)'),
      GetDC: user32.func('intptr_t __stdcall GetDC(intptr_t hwnd)'),
      ReleaseDC: user32.func('int __stdcall ReleaseDC(intptr_t hwnd, intptr_t hdc)'),
      GetDIBits: gdi32.func(
        'int __stdcall GetDIBits(intptr_t hdc, intptr_t bitmap, uint32_t start, uint32_t lines, void *bits, void *info, uint32_t usage)'
      ),
      DeleteObject: gdi32.func('bool __stdcall DeleteObject(intptr_t obj)')
    }
  } catch (err) {
    console.error('[winIcons] could not load Win32 APIs via koffi:', err)
    win32 = null
  }
  return win32
}

const BI_RGB = 0
const DIB_RGB_COLORS = 0

/** Reads a bitmap as top-down 32-bit BGRA. */
function readBitmap(
  w: Win32,
  hdc: number | bigint,
  bitmap: number | bigint,
  size: number
): Buffer | null {
  // BITMAPINFOHEADER (40 bytes) plus room for colour masks Windows may write.
  const info = Buffer.alloc(40 + 16)
  info.writeUInt32LE(40, 0) // biSize
  info.writeInt32LE(size, 4) // biWidth
  info.writeInt32LE(-size, 8) // biHeight (negative = top-down)
  info.writeUInt16LE(1, 12) // biPlanes
  info.writeUInt16LE(32, 14) // biBitCount
  info.writeUInt32LE(BI_RGB, 16) // biCompression
  const bits = Buffer.alloc(size * size * 4)
  const lines = w.GetDIBits(hdc, bitmap, 0, size, bits, info, DIB_RGB_COLORS)
  return lines === size ? bits : null
}

/**
 * Extracts icon `index` (or resource ID, when negative) from `file` at
 * `size`×`size` pixels. Returns null when the file has no such icon or this
 * isn't Windows.
 */
export function extractIcon(file: string, index: number, size: number): RawIcon | null {
  const w = api()
  if (!w || size <= 0) return null

  const icons: [number | bigint] = [0]
  const ids: [number] = [0]
  const count = w.PrivateExtractIconsW(file, index, size, size, icons, ids, 1, 0)
  const hicon = icons[0]
  // 0 = none found; 0xFFFFFFFF = file not found or not an icon container.
  if (count === 0 || count === 0xffffffff || !hicon) return null

  const info: Record<string, number | bigint> = {}
  const hdc = w.GetDC(0)
  try {
    if (!w.GetIconInfo(hicon, info)) return null
    try {
      if (!info.hbmColor) return null // monochrome icon; let the caller fall back
      const color = readBitmap(w, hdc, info.hbmColor, size)
      if (!color) return null

      // Icons without an alpha channel rely on the AND mask for transparency.
      let hasAlpha = false
      for (let i = 3; i < color.length; i += 4) {
        if (color[i] !== 0) {
          hasAlpha = true
          break
        }
      }
      if (hasAlpha) {
        // Icon bitmaps hold straight alpha; Skia wants it premultiplied.
        for (let i = 0; i < color.length; i += 4) {
          const a = color[i + 3]
          if (a === 255) continue
          color[i] = Math.round((color[i] * a) / 255)
          color[i + 1] = Math.round((color[i + 1] * a) / 255)
          color[i + 2] = Math.round((color[i + 2] * a) / 255)
        }
      } else {
        const mask = info.hbmMask ? readBitmap(w, hdc, info.hbmMask, size) : null
        for (let i = 0; i < color.length; i += 4) {
          // Mask: white = transparent, black = opaque.
          color[i + 3] = mask && mask[i] !== 0 ? 0 : 255
          if (color[i + 3] === 0) color[i] = color[i + 1] = color[i + 2] = 0
        }
      }
      return { width: size, height: size, data: color }
    } finally {
      if (info.hbmColor) w.DeleteObject(info.hbmColor)
      if (info.hbmMask) w.DeleteObject(info.hbmMask)
    }
  } finally {
    w.ReleaseDC(0, hdc)
    w.DestroyIcon(hicon)
  }
}
