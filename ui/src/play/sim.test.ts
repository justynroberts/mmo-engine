import { describe, expect, it } from 'vitest'
import { Interpolator, type Pos } from './interp'
import { integrate, Predictor, World } from './sim'

const walls = () => new World(256, [[100, 0, 110, 200], [0, 120, 90, 130]])

describe('prediction', () => {
  it('stops at walls like the server', () => {
    const w = walls()
    const p = new Predictor(95, 50, 20, 20, 0.5)
    for (let s = 1; s <= 100; s++) p.applyInput(w, s, 127, 0)
    expect(p.x).toBeLessThan(100)
  })

  // The same case as proto::world's built_blocks_stop_movement_like_walls_and_clearing_them_frees_the_way.
  it('stops at built blocks like the server, and passes once one is broken', () => {
    const w = new World(256, [])
    for (let cy = 0; cy < 128; cy++) w.setBlock(50, cy, 2)
    const walk = () => {
      let [x, y] = [95, 51]
      for (let i = 0; i < 200; i++) [x, y] = integrate(w, x, y, 127, 0, 20, 0.05, 0.5)
      return x
    }
    expect(walk()).toBeLessThan(100)
    expect(w.blocked(99.5, 50, 0.5)).toBe(false)
    expect(w.blocked(99.51, 50, 0.5)).toBe(true)
    w.setBlock(50, 25, 0)
    expect(walk()).toBeGreaterThan(102)
  })

  // The same case as proto::world's walkers_climb_a_staircase_one_block_at_a_time_and_drop_off_the_far_side.
  it('climbs a staircase one block at a time and drops off the far side, like the server', () => {
    const w = new World(256, [])
    for (const [cx, h] of [[50, 1], [51, 2], [52, 3], [53, 3]] as const) w.setBlock(cx, 25, h)
    let x = 95
    let top = 0
    for (let i = 0; i < 200; i++) {
      ;[x] = integrate(w, x, 51, 127, 0, 20, 0.05, 0.5)
      top = Math.max(top, w.level(x, 51, 0.5))
    }
    expect(top).toBe(3)
    expect(x).toBeGreaterThan(108)
    expect(w.level(x, 51, 0.5)).toBe(0)
    expect(w.blockedFrom(102, 51, 0.5, 0)).toBe(true)
    expect(w.blockedFrom(102, 51, 0.5, 1)).toBe(false)
    expect(w.blocked(101, 51, 0.5)).toBe(true)
    expect(w.blockedByWalls(101, 51, 0.5)).toBe(false)
  })

  it('does not move faster on diagonals', () => {
    const w = new World(512, [])
    const [x, y] = integrate(w, 100, 100, 127, 127, 20, 1, 0.5)
    expect(Math.hypot(x - 100, y - 100)).toBeCloseTo(20, 3)
  })

  it('replays unacknowledged inputs after the server position', () => {
    const w = walls()
    const p = new Predictor(50, 50, 20, 20, 0.5)
    for (let s = 1; s <= 10; s++) p.applyInput(w, s, 127, 0)
    const before = p.x
    const err = p.reconcile(w, 56, 50, 6)
    expect(p.unacked).toBe(4)
    expect(err).toBeLessThan(1e-3)
    expect(p.x).toBeCloseTo(before, 3)
  })

  it('reports a server-side teleport as a large correction', () => {
    const w = walls()
    const p = new Predictor(50, 50, 20, 20, 0.5)
    p.applyInput(w, 1, 127, 0)
    expect(p.reconcile(w, 200, 200, 1)).toBeGreaterThan(100)
    expect([p.x, p.y]).toEqual([200, 200])
  })

  it('survives sequence wraparound and bounds its memory', () => {
    const w = walls()
    const p = new Predictor(10, 10, 20, 20, 0.5)
    for (const s of [0xfffffffe, 0xffffffff, 0, 1]) p.applyInput(w, s, 0, 0)
    p.reconcile(w, 10, 10, 0xffffffff)
    expect(p.unacked).toBe(2)
    for (let s = 0; s < 1000; s++) p.applyInput(w, s, 0, 0)
    expect(p.unacked).toBe(64)
  })

  it('analog input moves proportionally', () => {
    const w = new World(512, [])
    const [x] = integrate(w, 100, 100, 64, 0, 20, 1, 0.5)
    expect(x - 100).toBeCloseTo(20 * (64 / 127), 2)
  })
})

describe('interpolation', () => {
  const ent = (id: number, x: number, y = 0): Pos => ({ id, x, y, attrs: { health: 50 } })
  // 20 Hz: one tick is 50 ms.
  it('blends between the snapshots either side of the drawn moment', () => {
    const i = new Interpolator(20, 2)
    i.push(10, [ent(1, 0)], 0)
    i.push(11, [ent(1, 10)], 50)
    i.push(12, [ent(1, 20)], 100)
    i.push(13, [ent(1, 30)], 150)
    // At t=150 the newest is tick 13; drawn two ticks behind, that is tick 11.
    expect(i.sample(150)[0]!.x).toBeCloseTo(10, 5)
    expect(i.sample(175)[0]!.x).toBeCloseTo(15, 5)
  })

  it('holds the newest snapshot when the stream stalls instead of extrapolating', () => {
    const i = new Interpolator(20, 2)
    i.push(1, [ent(1, 0)], 0)
    i.push(2, [ent(1, 10)], 50)
    expect(i.sample(5000)[0]!.x).toBe(10)
  })

  it('does not glide across a teleport', () => {
    const i = new Interpolator(20, 2, 48)
    i.push(1, [ent(1, 0)], 0)
    i.push(2, [ent(1, 400)], 50)
    i.push(3, [ent(1, 400)], 100)
    i.push(4, [ent(1, 400)], 150)
    const xs = [105, 115, 125, 135].map((t) => i.sample(t)[0]!.x)
    for (const x of xs) expect(x === 0 || x === 400).toBe(true)
  })

  it('shows a new entity at once and drops a removed one', () => {
    const i = new Interpolator(20, 2)
    i.push(1, [ent(1, 0)], 0)
    i.push(2, [ent(1, 5), ent(2, 7)], 50)
    i.push(3, [ent(2, 9)], 100)
    i.push(4, [ent(2, 9)], 150)
    const mid = i.sample(175) // drawn at tick 2.5, between ticks 2 and 3
    expect(mid.map((e) => e.id)).toEqual([2])
    expect(mid[0]!.x).toBeCloseTo(8, 5)
  })

  it('rides out a burst of late packets', () => {
    const i = new Interpolator(20, 2)
    // Ticks 1..5 arrive on time, then 6..9 all land together 200 ms late.
    for (let t = 1; t <= 5; t++) i.push(t, [ent(1, t * 10)], t * 50)
    for (let t = 6; t <= 9; t++) i.push(t, [ent(1, t * 10)], 500)
    let prev = -1
    for (let now = 300; now <= 700; now += 16) {
      const x = i.sample(now)[0]!.x
      expect(x).toBeGreaterThanOrEqual(prev)
      prev = x
    }
  })

  it('starts over when the server restarts', () => {
    const i = new Interpolator(20, 2)
    i.push(5000, [ent(1, 1)], 0)
    i.push(1, [ent(1, 2)], 100)
    expect(i.size).toBe(1)
  })
})
