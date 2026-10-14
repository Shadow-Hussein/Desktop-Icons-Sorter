import type { Bounds, ResizeEdge } from '../shared/types'
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
 * overlaps most, or the first one, and shrink it to fit if needed.
 */
export function clampToWorkAreas(b: Bounds, areas: Bounds[], visibleStrip = 40): Bounds {
  if (areas.length === 0) return b
  // The area overlapping the panel most, or else the one nearest to it.
  const centerDistance = (a: Bounds): number =>
    Math.hypot(a.x + a.width / 2 - (b.x + b.width / 2), a.y + a.height / 2 - (b.y + b.height / 2))
  const best = [...areas].sort(
    (a1, a2) =>
      intersectionArea(b, a2) - intersectionArea(b, a1) || centerDistance(a1) - centerDistance(a2)
  )[0]
  const titleStrip: Bounds = { x: b.x, y: b.y, width: b.width, height: Math.min(b.height, 30) }
  const reachable = areas.some((a) => intersectionArea(titleStrip, a) >= visibleStrip * 20)
  if (reachable) return b

  const width = Math.min(b.width, best.width)
  const height = Math.min(b.height, best.height)
  const x = Math.min(Math.max(b.x, best.x), best.x + best.width - width)
  const y = Math.min(Math.max(b.y, best.y), best.y + best.height - height)
  return { x, y, width, height }
}
