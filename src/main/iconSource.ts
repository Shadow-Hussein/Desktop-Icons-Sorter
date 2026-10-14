import { extname } from 'node:path'

/** Where a file's icon actually lives: a file with icon resources and an index into them. */
export interface IconSource {
  file: string
  /** Icon index, or a negative resource ID (the same convention as ExtractIconEx). */
  index: number
}

/** Files whose icon is stored inside the file itself. */
const ICON_CONTAINERS = new Set(['.exe', '.dll', '.ico', '.cpl', '.ocx', '.scr', '.icl'])

export function isIconContainer(path: string): boolean {
  return ICON_CONTAINERS.has(extname(path).toLowerCase())
}

/** Expands Windows-style `%VAR%` references (case-insensitive), leaving unknown ones as is. */
export function expandEnv(value: string, env: Record<string, string | undefined>): string {
  const lower = new Map(Object.entries(env).map(([k, v]) => [k.toLowerCase(), v]))
  return value.replace(
    /%([^%]+)%/g,
    (whole, name: string) => lower.get(name.toLowerCase()) ?? whole
  )
}

/** Reads `IconFile` / `IconIndex` from the `[InternetShortcut]` section of a .url file. */
export function parseUrlShortcut(text: string): { iconFile?: string; iconIndex: number } {
  let inSection = false
  let iconFile: string | undefined
  let iconIndex = 0
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    if (line.startsWith('[')) {
      inSection = line.toLowerCase() === '[internetshortcut]'
      continue
    }
    if (!inSection) continue
    const eq = line.indexOf('=')
    if (eq < 0) continue
    const key = line.slice(0, eq).trim().toLowerCase()
    const value = line.slice(eq + 1).trim()
    if (key === 'iconfile' && value) iconFile = value.replace(/^"(.*)"$/, '$1')
    else if (key === 'iconindex') iconIndex = Number.parseInt(value, 10) || 0
  }
  return { iconFile, iconIndex }
}

export interface ShortcutDetails {
  target?: string
  icon?: string
  iconIndex?: number
}

export interface IconSourceDeps {
  readShortcutLink(path: string): ShortcutDetails
  readFile(path: string): Promise<string>
  exists(path: string): Promise<boolean>
  env: Record<string, string | undefined>
}

/**
 * Finds the file that holds `path`'s real icon, the way Explorer does:
 * - .lnk: the shortcut's icon location, else its target
 * - .url: its IconFile/IconIndex (Steam and other launchers set these)
 * - .exe/.dll/.ico…: the file itself
 *
 * Returns null when there is nothing better than the generic icon (for
 * example a Store app shortcut with no target), so callers can fall back.
 */
export async function resolveIconSource(
  path: string,
  deps: IconSourceDeps
): Promise<IconSource | null> {
  const ext = extname(path).toLowerCase()
  const usable = async (file: string | undefined, index: number): Promise<IconSource | null> => {
    if (!file) return null
    const expanded = expandEnv(file, deps.env)
    return (await deps.exists(expanded)) ? { file: expanded, index } : null
  }

  if (ext === '.lnk') {
    let link: ShortcutDetails
    try {
      link = deps.readShortcutLink(path)
    } catch {
      return null
    }
    return (
      (await usable(link.icon, link.iconIndex ?? 0)) ??
      // No explicit icon: Explorer uses the target's own icon.
      (await usable(link.target, 0))
    )
  }

  if (ext === '.url') {
    let text: string
    try {
      text = await deps.readFile(path)
    } catch {
      return null
    }
    const { iconFile, iconIndex } = parseUrlShortcut(text)
    return usable(iconFile, iconIndex)
  }

  return isIconContainer(path) ? usable(path, 0) : null
}
