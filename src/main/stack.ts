import type { Bounds, CollapseDirection, PanelConfig } from '../shared/types'
import { SNAP_DISTANCE, visualBounds } from './geometry'

/** How far apart (or overlapping) two panels can be and still count as stacked. */
export const STACK_GAP = SNAP_DISTANCE
/** How much two stacked panels must overlap across the stack, so corners touching don't count. */
const MIN_OVERLAP = 16

const OPPOSITE: Record<CollapseDirection, CollapseDirection> = {
  up: 'down',
  down: 'up',
  left: 'right',
  right: 'left'
}

const isVertical = (side: CollapseDirection): boolean => side === 'up' || side === 'down'

/**
 * A panel's place in a stack that runs toward `side`: what it covers along the
 * stack, but its expanded extent across it, so a collapsed tile still holds up
 * the panels under its whole width.
 */
function stackRect(p: PanelConfig, side: CollapseDirection): Bounds {
  const v = visualBounds(p)
  return isVertical(side)
    ? { ...v, x: p.bounds.x, width: p.bounds.width }
    : { ...v, y: p.bounds.y, height: p.bounds.height }
}

function edge(r: Bounds, side: CollapseDirection): number {
  switch (side) {
    case 'up':
      return r.y
    case 'down':
      return r.y + r.height
    case 'left':
      return r.x
    case 'right':
      return r.x + r.width
  }
}

function overlap(a: Bounds, b: Bounds, vertical: boolean): number {
  return vertical
    ? Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)
    : Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y)
}

export interface StackMove {
  ids: string[]
  dx: number
  dy: number
}

/**
 * The panels to move when `before` collapses or expands into `after`: those
 * stacked against the edge opposite its bar (the edge that moves), then the
 * panels stacked against those, and so on, all moved as far as that edge did.
 */
export function stackMove(
  before: PanelConfig,
  after: PanelConfig,
  panels: PanelConfig[]
): StackMove {
  const side = OPPOSITE[before.collapseDirection]
  const vertical = isVertical(side)
  const shift = edge(stackRect(after, side), side) - edge(stackRect(before, side), side)
  if (shift === 0 || !before.stackable) return { ids: [], dx: 0, dy: 0 }

  // A panel that doesn't stack stays put and holds up nothing below it.
  const rest = new Map(
    panels.filter((p) => p.id !== before.id && p.stackable).map((p) => [p.id, stackRect(p, side)])
  )
  const ids: string[] = []
  const queue = [stackRect(before, side)]
  for (let r = queue.shift(); r; r = queue.shift()) {
    const far = edge(r, side)
    for (const [id, o] of rest) {
      const near = edge(o, OPPOSITE[side])
      if (Math.abs(near - far) <= STACK_GAP && overlap(r, o, vertical) >= MIN_OVERLAP) {
        ids.push(id)
        queue.push(o)
        rest.delete(id)
      }
    }
  }
  return vertical ? { ids, dx: 0, dy: shift } : { ids, dx: shift, dy: 0 }
}
