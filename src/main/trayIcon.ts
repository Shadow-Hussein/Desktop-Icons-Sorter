import { nativeImage, type NativeImage } from 'electron'

/**
 * Draws the tray icon (two stacked panels) at runtime, so the repo does not
 * need binary image assets. Returns a 32x32 image with a 16x16 representation.
 */
export function createTrayIcon(): NativeImage {
  const draw = (size: number): Buffer => {
    const buf = Buffer.alloc(size * size * 4) // BGRA
    const s = size / 16
    const rect = (x: number, y: number, w: number, h: number, bgra: number[]): void => {
      for (let py = Math.round(y * s); py < Math.round((y + h) * s); py++)
        for (let px = Math.round(x * s); px < Math.round((x + w) * s); px++) {
          const i = (py * size + px) * 4
          buf[i] = bgra[0]
          buf[i + 1] = bgra[1]
          buf[i + 2] = bgra[2]
          buf[i + 3] = bgra[3]
        }
    }
    const accent = [0xf0, 0x9c, 0x2e, 0xff] // #2e9cf0
    const fill = [0xf0, 0x9c, 0x2e, 0x70]
    const light = [0xff, 0xff, 0xff, 0xff]
    // Top panel: title bar + body + two icons.
    rect(1, 1, 14, 2, accent)
    rect(1, 3, 14, 5, fill)
    rect(3, 4, 3, 3, light)
    rect(8, 4, 3, 3, light)
    // Bottom panel, collapsed (title bar only).
    rect(1, 10, 14, 2, accent)
    rect(1, 13, 9, 2, accent)
    return buf
  }
  const img = nativeImage.createFromBitmap(draw(16), { width: 16, height: 16 })
  img.addRepresentation({ scaleFactor: 2, width: 32, height: 32, buffer: draw(32) })
  return img
}
