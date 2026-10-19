import type { PlacedItem } from './types'

/*
 * A panel's icon grid in custom order. Cells are numbered in reading order
 * (row * cols + col) for a grid `cols` wide, which depends on the panel's
 * width; saved spots are columns and rows, so they survive resizing.
 */

// Windows file names ignore case.
const key = (name: string): string => name.toLowerCase()

/**
 * The cell each item shows in, by name. `names` is every item, in the order
 * items without a spot should follow. An item keeps its saved spot unless
 * it's off the right edge (the panel got narrower) or already taken; then it
 * goes to the next free cell. Items without a spot follow the last occupied
 * cell, so gaps the user left stay empty.
 */
export function layoutGrid(
  names: string[],
  saved: PlacedItem[],
  cols: number
): Map<string, number> {
  const savedBy = new Map(saved.map((p) => [key(p.name), p]))
  const taken = new Set<number>()
  const out = new Map<string, number>()
  const put = (name: string, cell: number): void => {
    taken.add(cell)
    out.set(name, cell)
  }
  const nextFree = (from: number): number => {
    let cell = from
    while (taken.has(cell)) cell++
    return cell
  }

  const withSpot = names
    .map((name) => ({ name, spot: savedBy.get(key(name)) }))
    .filter((x): x is { name: string; spot: PlacedItem } => x.spot !== undefined)
    .sort((a, b) => a.spot.row - b.spot.row || a.spot.col - b.spot.col)
  const bumped: { name: string; from: number }[] = []
  for (const { name, spot } of withSpot) {
    const cell = spot.row * cols + spot.col
    if (spot.col < cols && !taken.has(cell)) put(name, cell)
    else bumped.push({ name, from: spot.row * cols + Math.min(spot.col, cols - 1) })
  }
  for (const { name, from } of bumped) put(name, nextFree(from))

  let last = -1
  for (const cell of taken) last = Math.max(last, cell)
  for (const name of names) {
    if (out.has(name)) continue
    last = nextFree(last + 1)
    put(name, last)
  }
  return out
}

/**
 * Puts `moving` (names, in order) into `target` and the cells after it, like
 * a phone's home screen: an item already there shifts one cell along with
 * the ones right after it, up to the first empty cell, so gaps elsewhere
 * stay. A moving item's old cell is freed first. Returns every item's spot.
 */
export function placeAt(
  layout: Map<string, number>,
  moving: string[],
  target: number,
  cols: number
): PlacedItem[] {
  const movingKeys = new Set(moving.map(key))
  const cells: (string | undefined)[] = []
  for (const [name, cell] of layout) if (!movingKeys.has(key(name))) cells[cell] = name
  let at = target
  for (const name of moving) {
    if (cells[at] !== undefined) {
      let end = at
      while (cells[end] !== undefined) end++
      for (let i = end; i > at; i--) cells[i] = cells[i - 1]
    }
    cells[at] = name
    at++
  }
  const out: PlacedItem[] = []
  cells.forEach((name, cell) => {
    if (name !== undefined) out.push({ name, col: cell % cols, row: Math.floor(cell / cols) })
  })
  return out
}
