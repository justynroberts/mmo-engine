import { describe, expect, it } from 'vitest'
import { CRAG, KEEP, LAKE, boxKind, rivers } from './mapLook'

// The console must draw the same rivers and box kinds as the Godot client. These numbers come from
// godot/scripts/terrain.gd for the same input (seed 7, a 1500 unit world, these 30 boxes); if either side
// changes its generator, both must change and this must be regenerated.
const BOXES: [number, number, number, number][] = [[1323.959, 193.297, 1368.851, 230.132], [81.189, 372.824, 96.714, 399.289], [571.141, 109.048, 603.348, 131.698], [32.283, 433.958, 44.501, 476.891], [1297.621, 1511.283, 1318.605, 1555.908], [1853.223, 1663.293, 1897.769, 1684.316], [1907.838, 1532.468, 1942.84, 1576.068], [1649.465, 44.064, 1660.374, 72.7], [1634.732, 1649.669, 1649.275, 1659.395], [1603.309, 1568.813, 1641.406, 1611.547], [13.26, 1567.259, 59.541, 1600.402], [842.814, 1326.24, 880.471, 1361.817], [217.082, 636.26, 264.393, 660.408], [1112.746, 1596.81, 1139.193, 1633.999], [140.387, 1493.387, 177.363, 1533.764], [729.298, 1008.002, 743.498, 1021.589], [1978.949, 505.11, 1998.904, 523.726], [1588.761, 1843.276, 1632.93, 1874.632], [374.702, 586.187, 403.023, 604.427], [1915.208, 1786.95, 1932.895, 1816.834], [1644.404, 2010.961, 1670.875, 2025.643], [23.061, 240.154, 38.748, 273.64], [1476.762, 1265.657, 1497.759, 1303.745], [766.545, 807.739, 801.437, 840.175], [1922.216, 550.348, 1961.831, 572.11], [994.604, 960.082, 1039.286, 994.924], [1693.612, 23.825, 1733.493, 37.15], [912.465, 922.347, 945.722, 936.717], [616.022, 1991.503, 645.836, 2030.375], [1919.981, 1347.808, 1928.511, 1375.961]]

describe('land features match the Godot client', () => {
  it('traces the same rivers', () => {
    const rs = rivers(7, 1500, BOXES)
    const godot = [
      { n: 158, mid: [704.1, 759.8], end: [494.91, 1503.27] },
      { n: 181, mid: [776.12, 769.81], end: [-6.95, 624.88] },
    ]
    expect(rs).toHaveLength(godot.length)
    rs.forEach((r, i) => {
      const g = godot[i]!
      expect(r).toHaveLength(g.n)
      const mid = r[Math.trunc(r.length / 2)]!
      const end = r[r.length - 1]!
      expect(mid[0]).toBeCloseTo(g.mid[0]!, 1)
      expect(mid[1]).toBeCloseTo(g.mid[1]!, 1)
      expect(end[0]).toBeCloseTo(g.end[0]!, 1)
      expect(end[1]).toBeCloseTo(g.end[1]!, 1)
    })
  })

  it('classifies boxes by their corner', () => {
    expect(boxKind([0, 0, 5, 40])).toBe(KEEP) // too thin to be anything else
    expect(boxKind([1, 0, 41, 40])).toBe(CRAG) // (1*7 + 0) % 10 = 7
    expect(boxKind([1, 7, 41, 47])).toBe(LAKE) // (7 + 91) % 10 = 8
    expect(boxKind([1, 1, 41, 41])).toBe(KEEP) // (7 + 13) % 10 = 0
  })
})
