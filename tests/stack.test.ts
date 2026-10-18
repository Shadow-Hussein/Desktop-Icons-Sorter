import { describe, expect, it } from 'vitest'
import { stackMove } from '../src/main/stack'
import { createPanel } from '../src/main/store'
import type { Bounds, PanelConfig } from '../src/shared/types'
import { FOLDER_CARD_SIZE, TITLE_BAR_HEIGHT } from '../src/shared/types'

function panel(id: string, bounds: Bounds, extra: Partial<PanelConfig> = {}): PanelConfig {
  return { ...createPanel({ title: id, folderPath: `/${id}`, bounds }), id, ...extra }
}

const collapse = (p: PanelConfig): PanelConfig => ({ ...p, collapsed: true })
const expand = (p: PanelConfig): PanelConfig => ({ ...p, collapsed: false })

// A column: a on top, b right under it, c under b with a small gap.
const a = panel('a', { x: 100, y: 100, width: 300, height: 200 })
const b = panel('b', { x: 100, y: 300, width: 300, height: 150 })
const c = panel('c', { x: 120, y: 456, width: 200, height: 100 })
const freed = 200 - TITLE_BAR_HEIGHT

describe('stackMove', () => {
  it('moves the panels below up when a panel collapses up', () => {
    expect(stackMove(a, collapse(a), [a, b, c])).toEqual({ ids: ['b', 'c'], dx: 0, dy: -freed })
  })

  it('moves them back down when it expands', () => {
    const collapsed = collapse(a)
    const movedB = { ...b, bounds: { ...b.bounds, y: b.bounds.y - freed } }
    const movedC = { ...c, bounds: { ...c.bounds, y: c.bounds.y - freed } }
    expect(stackMove(collapsed, expand(collapsed), [collapsed, movedB, movedC])).toEqual({
      ids: ['b', 'c'],
      dx: 0,
      dy: freed
    })
  })

  it('leaves panels above and beside alone', () => {
    const above = panel('above', { x: 100, y: 0, width: 300, height: 100 })
    const beside = panel('beside', { x: 400, y: 100, width: 200, height: 200 })
    const corner = panel('corner', { x: 400, y: 300, width: 200, height: 100 })
    expect(stackMove(a, collapse(a), [a, above, beside, corner]).ids).toEqual([])
  })

  it('stops at a gap wider than the snap distance', () => {
    const far = panel('far', { x: 100, y: 320, width: 300, height: 100 })
    expect(stackMove(a, collapse(a), [a, far]).ids).toEqual([])
  })

  it('moves the panels above down when a panel collapses down', () => {
    const lower = { ...b, collapseDirection: 'down' as const }
    expect(stackMove(lower, collapse(lower), [a, lower])).toEqual({
      ids: ['a'],
      dx: 0,
      dy: 150 - TITLE_BAR_HEIGHT
    })
  })

  it('works sideways', () => {
    const left = { ...a, id: 'left', collapseDirection: 'left' as const }
    const right = panel('right', { x: 400, y: 120, width: 200, height: 200 })
    expect(stackMove(left, collapse(left), [left, right])).toEqual({
      ids: ['right'],
      dx: -(300 - TITLE_BAR_HEIGHT),
      dy: 0
    })
  })

  it('holds up panels under the whole width of a collapsed tile', () => {
    const tile = { ...a, headerShape: 'folder' as const }
    // Its right part is outside the 122 px tile but under the panel's width.
    const offset = panel('offset', { x: 300, y: 300, width: 200, height: 100 })
    expect(stackMove(tile, collapse(tile), [tile, offset])).toEqual({
      ids: ['offset'],
      dx: 0,
      dy: FOLDER_CARD_SIZE - 200
    })
    const collapsed = collapse(tile)
    const raised = { ...offset, bounds: { ...offset.bounds, y: 100 + FOLDER_CARD_SIZE } }
    expect(stackMove(collapsed, expand(collapsed), [collapsed, raised]).ids).toEqual(['offset'])
  })

  it('skips panels that do not stack', () => {
    const fixed = { ...a, stackable: false }
    expect(stackMove(fixed, collapse(fixed), [fixed, b, c]).ids).toEqual([])
    // A panel left out of stacking stays put and holds up nothing under it.
    const loose = { ...b, stackable: false }
    expect(stackMove(a, collapse(a), [a, loose, c]).ids).toEqual([])
  })

  it('does nothing when the edge does not move', () => {
    // Collapsing a sideways panel doesn't move its bottom edge.
    const side = { ...a, collapseDirection: 'left' as const }
    const under = panel('under', { x: 100, y: 300, width: 300, height: 100 })
    expect(stackMove(side, collapse(side), [side, under]).ids).toEqual([])
  })
})
