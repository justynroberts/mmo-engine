import { describe, expect, it } from 'vitest'
import { fromBoxes, parseWorld } from './world'

// A 2x2 heightfield over [0, 10] x [0, 10]: heights 0, 10 along the top row and 20, 30 along the bottom.
function u16le(values: number[]): string {
  return btoa(String.fromCharCode(...values.flatMap((v) => [v & 255, v >> 8])))
}
const FILE = JSON.stringify({
  format: 1,
  size: 10,
  heights: { x0: 0, y0: 0, cell: 10, w: 2, h: 2, lo: 0, hi: 30, u16le: u16le([0, 21845, 43690, 65535]) },
  water: { rivers: [{ half_width: 5, pts: [[0, 5, 1], [10, 5, 0.5]] }], lakes: [{ b: [1, 1, 3, 3], level: -1 }] },
  objects: [{ t: 'house', b: [1, 1, 4, 4], solid: true, y: 2, v: 9 }, { t: 'tree', p: [5, 5], s: 1.5 }],
})

describe('the world file as the console reads it', () => {
  it('decodes the heights and interpolates between samples', () => {
    const w = parseWorld(FILE)!
    expect(w.hasHeights).toBe(true)
    expect(w.height(0, 0)).toBeCloseTo(0, 3)
    expect(w.height(10, 0)).toBeCloseTo(10, 2)
    expect(w.height(0, 10)).toBeCloseTo(20, 2)
    expect(w.height(5, 5)).toBeCloseTo(15, 2)
    expect(w.height(-50, 0)).toBeCloseTo(0, 3) // clamped at the edge
  })

  it('keeps the objects and water as they are', () => {
    const w = parseWorld(FILE)!
    expect(w.objects.map((o) => o.t)).toEqual(['house', 'tree'])
    expect(w.rivers[0]!.pts).toHaveLength(2)
    expect(w.lakes[0]!.level).toBe(-1)
  })

  it('refuses what is not a world file, and without one shows the walls on flat ground', () => {
    expect(parseWorld('nope')).toBeNull()
    expect(parseWorld(JSON.stringify({ format: 2, size: 10 }))).toBeNull()
    const w = fromBoxes(100, [[1, 2, 3, 4]])
    expect(w.objects).toEqual([{ t: 'block', b: [1, 2, 3, 4], solid: true, y: 0 }])
    expect(w.height(50, 50)).toBe(0)
  })
})
