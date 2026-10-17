import { describe, expect, it } from 'vitest'
import {
  clampToWorkAreas,
  collapsedBounds,
  expandedFromCollapsed,
  resizeBounds,
  snapToArea
} from '../src/main/geometry'
import { MIN_PANEL_HEIGHT, MIN_PANEL_WIDTH } from '../src/shared/types'

const start = { x: 100, y: 100, width: 300, height: 200 }

describe('resizeBounds', () => {
  it('grows from the south-east corner', () => {
    expect(resizeBounds(start, 'se', 50, 20)).toEqual({ x: 100, y: 100, width: 350, height: 220 })
  })
  it('keeps the opposite edge fixed when dragging north-west', () => {
    expect(resizeBounds(start, 'nw', 40, 30)).toEqual({ x: 140, y: 130, width: 260, height: 170 })
  })
  it('enforces the minimum size', () => {
    const b = resizeBounds(start, 'w', 1000, 0)
    expect(b.width).toBe(MIN_PANEL_WIDTH)
    expect(b.x + b.width).toBe(start.x + start.width)
    expect(resizeBounds(start, 's', 0, -1000).height).toBe(MIN_PANEL_HEIGHT)
  })
})

describe('snapToArea', () => {
  const area = { x: 0, y: 0, width: 1920, height: 1040 }
  it('snaps near edges', () => {
    expect(snapToArea({ ...start, x: 8, y: 5 }, area)).toMatchObject({ x: 0, y: 0 })
    expect(snapToArea({ ...start, x: 1615 }, area).x).toBe(1620)
  })
  it('leaves other positions alone', () => {
    expect(snapToArea(start, area)).toEqual(start)
  })
})

describe('clampToWorkAreas', () => {
  const areas = [
    { x: 0, y: 0, width: 1920, height: 1040 },
    { x: 1920, y: 0, width: 1280, height: 1000 }
  ]
  it('returns the same object when reachable', () => {
    const b = { x: 2000, y: 50, width: 300, height: 200 }
    expect(clampToWorkAreas(b, areas)).toBe(b)
  })
  it('pulls a panel back from an unplugged monitor onto the nearest one', () => {
    const b = clampToWorkAreas({ x: 4000, y: 50, width: 300, height: 200 }, areas)
    expect(b).toEqual({ x: 2900, y: 50, width: 300, height: 200 })
  })
  it('moves a panel whose title bar is above the screen', () => {
    const b = clampToWorkAreas({ x: 100, y: -500, width: 300, height: 200 }, areas)
    expect(b.y).toBe(0)
  })
  it('checks the bottom strip when the title bar is at the bottom', () => {
    // Top on screen, bottom (title bar) below it.
    const b = { x: 100, y: 900, width: 300, height: 400 }
    expect(clampToWorkAreas(b, areas)).toBe(b)
    expect(clampToWorkAreas(b, areas, 'down').y).toBe(640)
  })
  it('checks the side strip when the title bar is on the right', () => {
    // Left part on screen, right edge (title bar) past the last monitor.
    const b = { x: 3000, y: 50, width: 400, height: 200 }
    expect(clampToWorkAreas(b, areas)).toBe(b)
    expect(clampToWorkAreas(b, areas, 'right').x).toBe(2800)
  })
})

describe('collapsedBounds', () => {
  const expanded = { x: 10, y: 100, width: 300, height: 200 }
  it('keeps the top strip when collapsing up', () => {
    expect(collapsedBounds(expanded, 'up', 30)).toEqual({ x: 10, y: 100, width: 300, height: 30 })
  })
  it('keeps the bottom strip when collapsing down', () => {
    expect(collapsedBounds(expanded, 'down', 30)).toEqual({ x: 10, y: 270, width: 300, height: 30 })
  })
  it('keeps the left strip when collapsing left', () => {
    expect(collapsedBounds(expanded, 'left', 30)).toEqual({ x: 10, y: 100, width: 30, height: 200 })
  })
  it('keeps the right strip when collapsing right', () => {
    expect(collapsedBounds(expanded, 'right', 30)).toEqual({
      x: 280,
      y: 100,
      width: 30,
      height: 200
    })
  })
})

describe('expandedFromCollapsed', () => {
  const expanded = { x: 10, y: 100, width: 300, height: 200 }
  it('round-trips an unmoved panel in every direction', () => {
    for (const dir of ['up', 'down', 'left', 'right'] as const) {
      expect(expandedFromCollapsed(collapsedBounds(expanded, dir, 30), expanded, dir)).toEqual(
        expanded
      )
    }
  })
  it('moves the expanded panel with its collapsed bar', () => {
    const win = { x: 50, y: 500, width: 260, height: 30 }
    expect(expandedFromCollapsed(win, expanded, 'up')).toEqual({
      x: 50,
      y: 500,
      width: 260,
      height: 200
    })
    expect(expandedFromCollapsed(win, expanded, 'down')).toEqual({
      x: 50,
      y: 330,
      width: 260,
      height: 200
    })
  })
  it('anchors a down panel by its bottom edge when Windows kept the window taller', () => {
    // Bar bottom at 300, window 38 px tall instead of 30.
    const win = { x: 10, y: 262, width: 300, height: 38 }
    expect(expandedFromCollapsed(win, expanded, 'down').y).toBe(100)
  })
  it('moves and stretches a sideways panel with its vertical bar', () => {
    const win = { x: 600, y: 40, width: 30, height: 350 }
    expect(expandedFromCollapsed(win, expanded, 'left')).toEqual({
      x: 600,
      y: 40,
      width: 300,
      height: 350
    })
    expect(expandedFromCollapsed(win, expanded, 'right')).toEqual({
      x: 330,
      y: 40,
      width: 300,
      height: 350
    })
  })
})
