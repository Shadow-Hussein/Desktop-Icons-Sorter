import { execFile } from 'node:child_process'
import { nativeImage } from 'electron'
import { expandEnv, type IconSource } from './iconSource'
import { extractIcon } from './winIcons'

const SHELL_ICONS_KEY = 'SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Icons'

/** Windows' built-in shortcut arrow (used when no custom one is configured). */
export const DEFAULT_ARROW: IconSource = {
  file: '%SystemRoot%\\System32\\imageres.dll',
  index: -163
}

/**
 * Parses the `29` value (the shortcut-arrow overlay) out of `reg query`
 * output for the Shell Icons key. The value looks like `C:\path\file.ico,0`;
 * the index after the last comma is optional.
 */
export function parseShellIconValue(regOutput: string): IconSource | null {
  const match = /^\s*29\s+REG_(?:EXPAND_)?SZ\s+(.+?)\s*$/m.exec(regOutput)
  if (!match) return null
  const value = match[1].replace(/^"(.*)"$/, '$1')
  const comma = value.lastIndexOf(',')
  if (comma > 0 && /^\s*-?\d+\s*$/.test(value.slice(comma + 1))) {
    return {
      file: value
        .slice(0, comma)
        .trim()
        .replace(/^"(.*)"$/, '$1'),
      index: Number.parseInt(value.slice(comma + 1), 10)
    }
  }
  return value ? { file: value, index: 0 } : null
}

function queryShellIcon(hive: 'HKCU' | 'HKLM'): Promise<IconSource | null> {
  return new Promise((resolve) => {
    execFile(
      'reg.exe',
      ['query', `${hive}\\${SHELL_ICONS_KEY}`, '/v', '29'],
      { windowsHide: true, timeout: 5000 },
      (err, stdout) => resolve(err ? null : parseShellIconValue(stdout))
    )
  })
}

let source: Promise<IconSource> | null = null
const images = new Map<number, string | null>()

/** The arrow Explorer draws on shortcuts, honouring a custom `Shell Icons\29`. */
function overlaySource(): Promise<IconSource> {
  source ??= (async () => {
    const custom = (await queryShellIcon('HKCU')) ?? (await queryShellIcon('HKLM'))
    return custom ?? DEFAULT_ARROW
  })()
  return source
}

/**
 * Returns the shortcut-arrow overlay as a PNG data URL at `pixels` size.
 * Like Explorer's, it's a full-size transparent icon with the arrow in the
 * corner, meant to be drawn directly over the item's icon. Null off Windows,
 * or when the configured overlay is blank (people use that to hide arrows).
 */
export async function getShortcutOverlayDataUrl(pixels: number): Promise<string | null> {
  if (process.platform !== 'win32') return null
  if (images.has(pixels)) return images.get(pixels)!

  const { file, index } = await overlaySource()
  let raw = extractIcon(expandEnv(file, process.env), index, pixels)
  if (!raw && file !== DEFAULT_ARROW.file) {
    raw = extractIcon(expandEnv(DEFAULT_ARROW.file, process.env), DEFAULT_ARROW.index, pixels)
  }
  let url: string | null = null
  if (raw && raw.data.some((byte, i) => i % 4 === 3 && byte !== 0)) {
    url = nativeImage
      .createFromBitmap(raw.data, { width: raw.width, height: raw.height })
      .toDataURL()
  }
  images.set(pixels, url)
  return url
}
