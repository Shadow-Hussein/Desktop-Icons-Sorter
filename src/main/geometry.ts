import type {
  Bounds,
  CollapseAlign,
  CollapseDirection,
  PanelConfig,
  ResizeEdge
} from '../shared/types'
import {
  collapsedBarSize,
  collapsedLength,
  MIN_PANEL_HEIGHT,
  MIN_PANEL_WIDTH
} from '../shared/types'

export const SNAP_DISTANCE = 12

/** New bounds after dragging `edge` by (dx, dy), respecting minimum size. */
export function resizeBounds(start: Bounds, edge: ResizeEdge, dx: number, dy: number): Bounds {
  let { x, y, width, height } = start
  if (edge.includes('e')) width = Math.max(MIN_PANEL_WIDTH, start.width + dx)
  if (edge.includes('s')) height = Math.max(MIN_PANEL_HEIGHT, start.height + dy)
  if (edge.includes('w')) {
    width = Math.max(MIN_PANEL_WIDTH, start.width - dx)
    x = start.x + start.width - width
  }
  if (edge.includes('n')) {
    height = Math.max(MIN_PANEL_HEIGHT, start.height - dy)
    y = start.y + start.height - height
  }
  return {
    x: Math.round(x),
    y: Math.round(y),
    width: Math.round(width),
    height: Math.round(height)
  }
}

/** Left and right panels have a vertical title bar and collapse sideways. */
export function isSideways(direction: CollapseDirection): boolean {
  return direction === 'left' || direction === 'right'
}

/** How far a part of length `part` sits from the start of `full` when aligned to `align`. */
function alignOffset(full: number, part: number, align: CollapseAlign): number {
  if (align === 'start') return 0
  return align === 'center' ? Math.round((full - part) / 2) : full - part
}

/**
 * The window of a collapsed panel: the strip along the bar's edge, `barSize`
 * deep. `length` shortens it along the bar, as for a square folder card, and
 * `align` places it at the start, middle or end; null keeps the full length.
 */
export function collapsedBounds(
  expanded: Bounds,
  direction: CollapseDirection,
  barSize: number,
  length: number | null = null,
  align: CollapseAlign = 'start'
): Bounds {
  const { x, y, width, height } = expanded
  const w = length ?? width
  const h = length ?? height
  const dx = alignOffset(width, w, align)
  const dy = alignOffset(height, h, align)
  switch (direction) {
    case 'up':
      return { x: x + dx, y, width: w, height: barSize }
    case 'down':
      return { x: x + dx, y: y + height - barSize, width: w, height: barSize }
    case 'left':
      return { x, y: y + dy, width: barSize, height: h }
    case 'right':
      return { x: x + width - barSize, y: y + dy, width: barSize, height: h }
  }
}

/**
 * Expanded bounds for a collapsed panel whose window was moved or resized to
 * `win`. Uses the window's edge on the bar's side, since Windows may keep a
 * collapsed window larger than the bar. With a fixed `length` (a square
 * folder card) the window's length along the bar says nothing about the
 * panel's, and `align` says where along the panel the card sat.
 */
export function expandedFromCollapsed(
  win: Bounds,
  expanded: Bounds,
  direction: CollapseDirection,
  length: number | null = null,
  align: CollapseAlign = 'start'
): Bounds {
  const width = length === null ? win.width : expanded.width
  const height = length === null ? win.height : expanded.height
  const x = length === null ? win.x : win.x - alignOffset(expanded.width, length, align)
  const y = length === null ? win.y : win.y - alignOffset(expanded.height, length, align)
  switch (direction) {
    case 'up':
      return { ...expanded, x, y: win.y, width }
    case 'down':
      return { ...expanded, x, y: win.y + win.height - expanded.height, width }
    case 'left':
      return { ...expanded, x: win.x, y, height }
    case 'right':
      return { ...expanded, x: win.x + win.width - expanded.width, y, height }
  }
}

/**
 * What a panel covers on screen: its expanded bounds, or the bar it collapsed
 * into. Its window can be larger (see PanelManager.placeWindow), but the rest
 * of the window is transparent.
 */
export function visualBounds(p: PanelConfig): Bounds {
  if (!p.collapsed) return p.bounds
  return collapsedBounds(
    p.bounds,
    p.collapseDirection,
    collapsedBarSize(p.headerShape),
    collapsedLength(p.headerShape),
    p.collapseAlign
  )
}

/** Vertical (x) and horizontal (y) lines a moving or resizing panel snaps to. */
export interface SnapLines {
  x: number[]
  y: number[]
}

/** The edges of every rect in `rects`, e.g. other panels and the work area. */
export function snapLines(rects: Bounds[]): SnapLines {
  return {
    x: rects.flatMap((r) => [r.x, r.x + r.width]),
    y: rects.flatMap((r) => [r.y, r.y + r.height])
  }
}

/** The smallest shift that puts one of `values` on one of `lines`, or 0 if none is close. */
function snapShift(values: number[], lines: number[], distance: number): number {
  let best = 0
  let bestAbs = distance + 1
  for (const v of values) {
    for (const line of lines) {
      const d = line - v
      if (Math.abs(d) < bestAbs) {
        best = d
        bestAbs = Math.abs(d)
      }
    }
  }
  return bestAbs <= distance ? best : 0
}

/**
 * Snap a rect being moved so its nearest edge on each axis lands on a line
 * within `distance`. The rect comes from the cursor each time, so it sticks to
 * a line until the cursor has moved `distance` past it.
 */
export function snapMove(b: Bounds, lines: SnapLines, distance = SNAP_DISTANCE): Bounds {
  return {
    ...b,
    x: b.x + snapShift([b.x, b.x + b.width], lines.x, distance),
    y: b.y + snapShift([b.y, b.y + b.height], lines.y, distance)
  }
}

/** Snap the edges named by `edge` (as in resizeBounds) to lines, keeping the minimum size. */
export function snapResize(
  b: Bounds,
  edge: string,
  lines: SnapLines,
  distance = SNAP_DISTANCE
): Bounds {
  let { x, y, width, height } = b
  const right = x + width
  const bottom = y + height
  if (edge.includes('e')) {
    const r = right + snapShift([right], lines.x, distance)
    if (r - x >= MIN_PANEL_WIDTH) width = r - x
  } else if (edge.includes('w')) {
    const l = x + snapShift([x], lines.x, distance)
    if (right - l >= MIN_PANEL_WIDTH) [x, width] = [l, right - l]
  }
  if (edge.includes('s')) {
    const btm = bottom + snapShift([bottom], lines.y, distance)
    if (btm - y >= MIN_PANEL_HEIGHT) height = btm - y
  } else if (edge.includes('n')) {
    const t = y + snapShift([y], lines.y, distance)
    if (bottom - t >= MIN_PANEL_HEIGHT) [y, height] = [t, bottom - t]
  }
  return { x, y, width, height }
}

function intersectionArea(a: Bounds, b: Bounds): number {
  const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)
  const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y)
  return w > 0 && h > 0 ? w * h : 0
}

/**
 * Make sure a panel is reachable: if less than a title-bar-sized strip is on
 * any work area (e.g. a monitor was unplugged), move it onto the work area it
 * overlaps most, or the first one, and shrink it to fit if needed. The strip
 * runs along the edge the title bar sits on.
 */
export function clampToWorkAreas(
  b: Bounds,
  areas: Bounds[],
  titleSide: CollapseDirection = 'up',
  visibleStrip = 40
): Bounds {
  if (areas.length === 0) return b
  // The area overlapping the panel most, or else the one nearest to it.
  const centerDistance = (a: Bounds): number =>
    Math.hypot(a.x + a.width / 2 - (b.x + b.width / 2), a.y + a.height / 2 - (b.y + b.height / 2))
  const best = [...areas].sort(
    (a1, a2) =>
      intersectionArea(b, a2) - intersectionArea(b, a1) || centerDistance(a1) - centerDistance(a2)
  )[0]
  const titleStrip = collapsedBounds(
    b,
    titleSide,
    Math.min(isSideways(titleSide) ? b.width : b.height, 30)
  )
  const reachable = areas.some((a) => intersectionArea(titleStrip, a) >= visibleStrip * 20)
  if (reachable) return b

  const width = Math.min(b.width, best.width)
  const height = Math.min(b.height, best.height)
  const x = Math.min(Math.max(b.x, best.x), best.x + best.width - width)
  const y = Math.min(Math.max(b.y, best.y), best.y + best.height - height)
  return { x, y, width, height }
}
