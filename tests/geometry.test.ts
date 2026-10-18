import { describe, expect, it } from 'vitest'
import {
  clampToWorkAreas,
  collapsedBounds,
  expandedFromCollapsed,
  resizeBounds,
  snapLines,
  snapMove,
  snapResize,
  visualBounds
} from '../src/main/geometry'
import { createPanel } from '../src/main/store'
import {
  collapsedBarSize,
  collapsedLength,
  MIN_PANEL_HEIGHT,
  MIN_PANEL_WIDTH
} from '../src/shared/types'

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

describe('snapMove', () => {
  const area = snapLines([{ x: 0, y: 0, width: 1920, height: 1040 }])
  it('snaps near screen edges', () => {
    expect(snapMove({ ...start, x: 8, y: 5 }, area)).toMatchObject({ x: 0, y: 0 })
    expect(snapMove({ ...start, x: 1615 }, area).x).toBe(1620)
  })
  it('leaves other positions alone', () => {
    expect(snapMove(start, area)).toEqual(start)
  })
  it('lines up with another panel and sits against it', () => {
    const lines = snapLines([{ x: 500, y: 100, width: 300, height: 200 }])
    // Left edges line up.
    expect(snapMove({ ...start, x: 507, y: 600 }, lines)).toMatchObject({ x: 500, y: 600 })
    // Top edge against the other panel's bottom edge, right edge against its left.
    expect(snapMove({ ...start, x: 190, y: 290 }, lines)).toMatchObject({ x: 200, y: 300 })
  })
  it('picks the closest line', () => {
    const lines = { x: [95, 410], y: [] }
    // Left edge 5 px from 95, right edge 10 px from 410.
    expect(snapMove(start, lines).x).toBe(95)
  })
  it('lets go once the panel is moved past the snap distance', () => {
    const lines = { x: [500], y: [] }
    expect(snapMove({ ...start, x: 512 }, lines).x).toBe(500)
    expect(snapMove({ ...start, x: 513 }, lines).x).toBe(513)
  })
})

describe('snapResize', () => {
  const lines = { x: [405, 95], y: [310] }
  it('snaps only the dragged edges', () => {
    expect(snapResize(start, 'e', lines)).toEqual({ ...start, width: 305 })
    expect(snapResize(start, 'w', lines)).toEqual({ ...start, x: 95, width: 305 })
    expect(snapResize(start, 'se', lines)).toEqual({ ...start, width: 305, height: 210 })
    expect(snapResize(start, 'n', lines)).toEqual(start)
  })
  it('keeps the minimum size', () => {
    const small = { x: 0, y: 0, width: MIN_PANEL_WIDTH + 5, height: 200 }
    expect(snapResize(small, 'e', { x: [MIN_PANEL_WIDTH - 5], y: [] }, 12)).toEqual(small)
  })
})

describe('visualBounds', () => {
  const panel = createPanel({ title: 'A', folderPath: '/a', bounds: start })
  it('is the panel bounds while expanded', () => {
    expect(visualBounds(panel)).toEqual(start)
  })
  it('is the bar, not the larger window, once collapsed', () => {
    expect(visualBounds({ ...panel, collapsed: true })).toEqual({ ...start, height: 30 })
    expect(visualBounds({ ...panel, collapsed: true, collapseDirection: 'down' })).toEqual({
      ...start,
      y: 270,
      height: 30
    })
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
  it('keeps the taller strip of a phone panel on the bar edge', () => {
    const size = collapsedBarSize('phone')
    expect(size).toBeGreaterThan(collapsedBarSize('default'))
    expect(collapsedBounds(expanded, 'down', size)).toEqual({
      x: 10,
      y: 300 - size,
      width: 300,
      height: size
    })
    expect(collapsedBounds(expanded, 'right', size)).toEqual({
      x: 310 - size,
      y: 100,
      width: size,
      height: 200
    })
  })
})

describe('collapsedBounds for a folder card', () => {
  const expanded = { x: 10, y: 100, width: 300, height: 200 }
  const size = collapsedBarSize('folder')
  const length = collapsedLength('folder')
  it('is a square', () => {
    expect(length).toBe(size)
  })
  it('sits in the corner on the bar edge, at the top or left end', () => {
    expect(collapsedBounds(expanded, 'up', size, length)).toEqual({
      x: 10,
      y: 100,
      width: size,
      height: size
    })
    expect(collapsedBounds(expanded, 'down', size, length)).toEqual({
      x: 10,
      y: 300 - size,
      width: size,
      height: size
    })
    expect(collapsedBounds(expanded, 'left', size, length)).toEqual({
      x: 10,
      y: 100,
      width: size,
      height: size
    })
    expect(collapsedBounds(expanded, 'right', size, length)).toEqual({
      x: 310 - size,
      y: 100,
      width: size,
      height: size
    })
  })
  it('can sit in the middle or at the end of the bar edge', () => {
    // 300 x 200 panel, 122 px square: 89 px in from the left when centered.
    expect(collapsedBounds(expanded, 'up', size, length, 'center')).toMatchObject({
      x: 10 + 89,
      y: 100
    })
    expect(collapsedBounds(expanded, 'down', size, length, 'end')).toMatchObject({
      x: 310 - size,
      y: 300 - size
    })
    expect(collapsedBounds(expanded, 'left', size, length, 'center')).toMatchObject({
      x: 10,
      y: 100 + 39
    })
    expect(collapsedBounds(expanded, 'right', size, length, 'end')).toMatchObject({
      x: 310 - size,
      y: 300 - size
    })
  })
  it('ignores the position for a full-length strip', () => {
    expect(collapsedBounds(expanded, 'up', 30, null, 'end')).toEqual(
      collapsedBounds(expanded, 'up', 30)
    )
  })
})

describe('expandedFromCollapsed', () => {
  const expanded = { x: 10, y: 100, width: 300, height: 200 }
  it('round-trips an unmoved panel in every direction', () => {
    for (const dir of ['up', 'down', 'left', 'right'] as const) {
      for (const size of [30, collapsedBarSize('phone')]) {
        expect(expandedFromCollapsed(collapsedBounds(expanded, dir, size), expanded, dir)).toEqual(
          expanded
        )
      }
    }
  })
  it('round-trips an unmoved folder card in every direction and position', () => {
    const size = collapsedBarSize('folder')
    const length = collapsedLength('folder')
    for (const dir of ['up', 'down', 'left', 'right'] as const) {
      for (const align of ['start', 'center', 'end'] as const) {
        const card = collapsedBounds(expanded, dir, size, length, align)
        expect(expandedFromCollapsed(card, expanded, dir, length, align)).toEqual(expanded)
      }
    }
  })
  it('keeps the expanded size when a folder card is moved', () => {
    const win = { x: 600, y: 400, width: 122, height: 122 }
    const length = collapsedLength('folder')
    expect(expandedFromCollapsed(win, expanded, 'up', length)).toEqual({
      x: 600,
      y: 400,
      width: 300,
      height: 200
    })
    expect(expandedFromCollapsed(win, expanded, 'right', length)).toEqual({
      x: 422,
      y: 400,
      width: 300,
      height: 200
    })
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
