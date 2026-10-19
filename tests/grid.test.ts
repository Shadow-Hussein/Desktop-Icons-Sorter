import { describe, expect, it } from 'vitest'
import { layoutGrid, placeAt } from '../src/shared/grid'

const asObject = (m: Map<string, number>): Record<string, number> => Object.fromEntries(m)

describe('layoutGrid', () => {
  it('packs items without spots, in the order given', () => {
    expect(asObject(layoutGrid(['a', 'b', 'c'], [], 2))).toEqual({ a: 0, b: 1, c: 2 })
  })

  it('keeps saved spots and the gaps between them', () => {
    const saved = [
      { name: 'a', col: 0, row: 0 },
      { name: 'b', col: 3, row: 1 }
    ]
    expect(asObject(layoutGrid(['a', 'b'], saved, 4))).toEqual({ a: 0, b: 7 })
  })

  it('puts items without a spot after the last occupied cell, leaving gaps alone', () => {
    const saved = [{ name: 'a', col: 2, row: 0 }]
    expect(asObject(layoutGrid(['a', 'new1', 'new2'], saved, 4))).toEqual({
      a: 2,
      new1: 3,
      new2: 4
    })
  })

  it('moves an item off the right edge to the next free cell, and back when wider', () => {
    const saved = [
      { name: 'a', col: 0, row: 0 },
      { name: 'far', col: 4, row: 0 }
    ]
    expect(asObject(layoutGrid(['a', 'far'], saved, 3))).toEqual({ a: 0, far: 2 })
    expect(asObject(layoutGrid(['a', 'far'], saved, 5))).toEqual({ a: 0, far: 4 })
  })

  it('resolves two items saved in the same cell', () => {
    const saved = [
      { name: 'a', col: 1, row: 0 },
      { name: 'b', col: 1, row: 0 }
    ]
    expect(asObject(layoutGrid(['a', 'b'], saved, 3))).toEqual({ a: 1, b: 2 })
  })

  it('matches saved names regardless of case', () => {
    expect(asObject(layoutGrid(['Game.lnk'], [{ name: 'game.LNK', col: 2, row: 1 }], 3))).toEqual({
      'Game.lnk': 5
    })
  })
})

describe('placeAt', () => {
  const layout = new Map([
    ['a', 0],
    ['b', 1],
    ['c', 2],
    ['d', 5]
  ])

  it('puts an item into an empty cell and leaves its old cell empty', () => {
    expect(placeAt(layout, ['a'], 7, 4)).toEqual([
      { name: 'b', col: 1, row: 0 },
      { name: 'c', col: 2, row: 0 },
      { name: 'd', col: 1, row: 1 },
      { name: 'a', col: 3, row: 1 }
    ])
  })

  it('shifts the run of items at the target up to the first gap only', () => {
    // x lands on b's cell: b and c move along into cell 3; d (after a gap) stays.
    const out = placeAt(layout, ['x'], 1, 4)
    expect(Object.fromEntries(out.map((p) => [p.name, p.row * 4 + p.col]))).toEqual({
      a: 0,
      x: 1,
      b: 2,
      c: 3,
      d: 5
    })
  })

  it('lets a moved item take a spot earlier in its own run', () => {
    // c moves to a's cell: a and b shift into the cell c left.
    const out = placeAt(layout, ['c'], 0, 4)
    expect(Object.fromEntries(out.map((p) => [p.name, p.row * 4 + p.col]))).toEqual({
      c: 0,
      a: 1,
      b: 2,
      d: 5
    })
  })

  it('places several items one after another', () => {
    const out = placeAt(new Map([['a', 0]]), ['x', 'y'], 4, 3)
    expect(out).toEqual([
      { name: 'a', col: 0, row: 0 },
      { name: 'x', col: 1, row: 1 },
      { name: 'y', col: 2, row: 1 }
    ])
  })
})
