import type { Bounds, CollapseDirection, ResizeEdge } from '../shared/types'
import { MIN_PANEL_HEIGHT, MIN_PANEL_WIDTH } from '../shared/types'

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

/** The title-bar-only window of a collapsed panel: the strip along the bar's edge. */
export function collapsedBounds(
  expanded: Bounds,
  direction: CollapseDirection,
  barSize: number
): Bounds {
  const { x, y, width, height } = expanded
  switch (direction) {
    case 'up':
      return { x, y, width, height: barSize }
    case 'down':
      return { x, y: y + height - barSize, width, height: barSize }
    case 'left':
      return { x, y, width: barSize, height }
    case 'right':
      return { x: x + width - barSize, y, width: barSize, height }
  }
}

/**
 * Expanded bounds for a collapsed panel whose window was moved or resized to
 * `win`. Uses the window's edge on the bar's side, since Windows may keep a
 * collapsed window larger than the bar.
 */
export function expandedFromCollapsed(
  win: Bounds,
  expanded: Bounds,
  direction: CollapseDirection
): Bounds {
  switch (direction) {
    case 'up':
      return { ...expanded, x: win.x, y: win.y, width: win.width }
    case 'down':
      return { ...expanded, x: win.x, y: win.y + win.height - expanded.height, width: win.width }
    case 'left':
      return { ...expanded, x: win.x, y: win.y, height: win.height }
    case 'right':
      return { ...expanded, x: win.x + win.width - expanded.width, y: win.y, height: win.height }
  }
}

/** Snap a window being moved to the edges of `area` (e.g. a display work area). */
export function snapToArea(b: Bounds, area: Bounds, distance = SNAP_DISTANCE): Bounds {
  let { x, y } = b
  if (Math.abs(x - area.x) <= distance) x = area.x
  else if (Math.abs(x + b.width - (area.x + area.width)) <= distance)
    x = area.x + area.width - b.width
  if (Math.abs(y - area.y) <= distance) y = area.y
  else if (Math.abs(y + b.height - (area.y + area.height)) <= distance)
    y = area.y + area.height - b.height
  return { ...b, x, y }
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
