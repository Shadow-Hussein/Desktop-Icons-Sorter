import { describe, expect, it } from 'vitest'
import { clampToWorkAreas, resizeBounds, snapToArea } from '../src/main/geometry'
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
})
