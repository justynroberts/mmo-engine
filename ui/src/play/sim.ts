// The movement step the server runs, so the browser can predict its own character instead of waiting a round
// trip to see a keypress. It mirrors crates/proto/src/world.rs and predict.rs. The server uses f32, so the
// arithmetic is rounded to f32 at each step (Math.fround); where the two still disagree the next snapshot
// corrects it, which is what reconcile() is for.

export type Box = readonly [number, number, number, number]

const f = Math.fround
const CELL = 64
/** Edge of a building block (proto::world::BLOCK); layer z fills heights z * BLOCK to (z + 1) * BLOCK. */
export const BLOCK = 2
/** Layers a column spans above its base (proto::world::COLUMN_LAYERS). */
export const COLUMN_LAYERS = 16
/** The layer of something on the bare ground (proto::world::GROUND). */
export const GROUND = -128
/** Layers a walker steps up without help (proto::world::CLIMB); it drops off any height. */
export const CLIMB = 1
const LAST_CELL = (1 << 15) - 1
/** Edge of a road cell (proto::world::ROAD_CELL), and the road speed when the server does not say. */
export const ROAD_CELL = 4
export const ROAD_SPEED = 1.5
/** A road cell's key (proto::world::road_key). */
export const roadKey = (cx: number, cy: number) => cx + cy * 65536

/** A built column (proto::world::Column): its base layer and a bit per layer above it. */
export interface Column {
  base: number
  mask: number
}

const solidIn = (c: Column, z: number) => {
  const k = z - c.base
  return k >= 0 && k < COLUMN_LAYERS && (c.mask & (1 << k)) !== 0
}

export class World {
  readonly size: number
  private boxes: Box[]
  private gw: number
  private buckets: Map<number, number[]>
  /** The built block columns, by cy << 15 | cx; never with an empty mask. */
  private blocks = new Map<number, Column>()
  /** The road cells (roadKey), and how much faster movement is on them (the game's road_speed). */
  private roads: Set<number>
  readonly roadSpeed: number

  constructor(size: number, boxes: readonly Box[], roads: Iterable<number> = [], roadSpeed = ROAD_SPEED) {
    this.size = f(size)
    this.roads = new Set(roads)
    this.roadSpeed = f(roadSpeed)
    this.boxes = boxes.map((b) => [f(b[0]), f(b[1]), f(b[2]), f(b[3])] as const)
    this.gw = Math.ceil(this.size / CELL) + 1
    // Sparse: a 65,536 unit world has a million cells and only a few hold walls.
    this.buckets = new Map()
    this.boxes.forEach((b, i) => {
      const cx0 = Math.max(0, Math.floor(b[0] / CELL))
      const cx1 = Math.min(this.gw - 1, Math.floor(b[2] / CELL))
      const cy0 = Math.max(0, Math.floor(b[1] / CELL))
      const cy1 = Math.min(this.gw - 1, Math.floor(b[3] / CELL))
      for (let cy = cy0; cy <= cy1; cy++) for (let cx = cx0; cx <= cx1; cx++) {
        const k = cy * this.gw + cx
        const l = this.buckets.get(k)
        if (l) l.push(i)
        else this.buckets.set(k, [i])
      }
    })
  }

  /** True if the point is on a road cell (proto World::on_road). */
  onRoad(x: number, y: number): boolean {
    return this.roads.size > 0 && x >= 0 && y >= 0 && this.roads.has(roadKey(Math.floor(x / ROAD_CELL), Math.floor(y / ROAD_CELL)))
  }

  /** Set a column; an empty mask clears it. */
  setColumn(cx: number, cy: number, col: Column) {
    if (cx < 0 || cy < 0 || cx > LAST_CELL || cy > LAST_CELL) return
    if (col.mask & 0xffff) this.blocks.set((cy << 15) | cx, { base: col.base, mask: col.mask & 0xffff })
    else this.blocks.delete((cy << 15) | cx)
  }

  column(cx: number, cy: number): Column | undefined {
    return this.blocks.get((cy << 15) | cx)
  }

  private under(x: number, y: number, r: number, fn: (c: Column) => void) {
    if (this.blocks.size === 0) return
    const cx0 = Math.max(0, Math.floor(f(x - r) / BLOCK))
    const cx1 = Math.min(LAST_CELL, Math.floor(f(x + r) / BLOCK))
    const cy0 = Math.max(0, Math.floor(f(y - r) / BLOCK))
    const cy1 = Math.min(LAST_CELL, Math.floor(f(y + r) / BLOCK))
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        const c = this.blocks.get((cy << 15) | cx)
        if (!c) continue
        const bx = cx * BLOCK
        const by = cy * BLOCK
        if (f(x + r) > bx && f(x - r) < bx + BLOCK && f(y + r) > by && f(y - r) < by + BLOCK) fn(c)
      }
    }
  }

  /** The highest base of the columns under a circle, or null away from all of them. */
  floor(x: number, y: number, r: number): number | null {
    let top: number | null = null
    this.under(x, y, r, (c) => {
      if (top === null || c.base > top) top = c.base
    })
    return top
  }

  solidAt(x: number, y: number, r: number, z: number): boolean {
    let hit = false
    this.under(x, y, r, (c) => {
      hit ||= solidIn(c, z)
    })
    return hit
  }

  /** Where feet in layer z end up without moving sideways: up out of any block, then down onto what is below. */
  settle(x: number, y: number, r: number, z: number): number {
    const floor = this.floor(x, y, r)
    if (floor === null) return GROUND
    let at = z === GROUND ? floor : Math.max(z, floor)
    while (this.solidAt(x, y, r, at) && at < 127) at++
    while (at > floor && !this.solidAt(x, y, r, at - 1)) at--
    return at
  }

  /** Where feet in layer z stand after moving to (x, y), or null if they cannot go there. */
  enter(x: number, y: number, r: number, z: number): number | null {
    if (this.blockedByWalls(x, y, r)) return null
    const floor = this.floor(x, y, r)
    if (floor === null) return GROUND
    let at = z === GROUND ? floor : Math.max(z, floor)
    if (this.solidAt(x, y, r, at)) {
      for (let up = 1; up <= CLIMB; up++) {
        if (!this.solidAt(x, y, r, at + up)) return this.settle(x, y, r, at + up)
      }
      return null
    }
    return this.settle(x, y, r, at)
  }

  /** True if a circle (treated as its bounding square) at (x, y) leaves the world or overlaps a wall or column. */
  blocked(x: number, y: number, r: number): boolean {
    return this.floor(x, y, r) !== null || this.blockedByWalls(x, y, r)
  }

  blockedByWalls(x: number, y: number, r: number): boolean {
    const size = this.size
    if (f(x - r) < 0 || f(y - r) < 0 || f(x + r) >= size || f(y + r) >= size) return true
    const cx0 = Math.floor(f(x - r) / CELL)
    const cx1 = Math.min(Math.floor(f(x + r) / CELL), this.gw - 1)
    const cy0 = Math.floor(f(y - r) / CELL)
    const cy1 = Math.min(Math.floor(f(y + r) / CELL), this.gw - 1)
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        for (const bi of this.buckets.get(cy * this.gw + cx) ?? []) {
          const b = this.boxes[bi]!
          if (f(x + r) > b[0] && f(x - r) < b[2] && f(y + r) > b[1] && f(y - r) < b[3]) return true
        }
      }
    }
    return false
  }

  /** Axis-separated move so a character slides along a wall instead of sticking to it, climbing and dropping
   *  as it goes. */
  step(x: number, y: number, z: number, dx: number, dy: number, r: number): [number, number, number] {
    const nx = f(x + dx)
    const zx = this.enter(nx, y, r, z)
    const [x1, z1] = zx === null ? [x, z] : [nx, zx]
    const ny = f(y + dy)
    const zy = this.enter(x1, ny, r, z1)
    const [y1, z2] = zy === null ? [y, z1] : [ny, zy]
    return [x1, y1, z2]
  }
}

/** Wire direction (-127..127 per axis) to a vector of length at most 1. */
export function moveVector(dirX: number, dirY: number): [number, number] {
  const fx = Math.max(-1, Math.min(1, f(dirX / 127)))
  const fy = Math.max(-1, Math.min(1, f(dirY / 127)))
  const len2 = f(f(fx * fx) + f(fy * fy))
  if (len2 > 1) {
    const inv = f(1 / f(Math.sqrt(len2)))
    return [f(fx * inv), f(fy * inv)]
  }
  return [fx, fy]
}

export function integrate(world: World, x: number, y: number, z: number, dirX: number, dirY: number, speed: number, dt: number, r: number): [number, number, number] {
  const [mx, my] = moveVector(dirX, dirY)
  if (mx === 0 && my === 0) return [x, y, z === GROUND ? z : world.settle(x, y, r, z)]
  const s = world.onRoad(x, y) ? f(speed * world.roadSpeed) : speed
  return world.step(x, y, z, f(f(mx * s) * dt), f(f(my * s) * dt), r)
}

/** Inputs kept for replay: over three seconds at 20 Hz. */
const MAX_PENDING = 64

export class Predictor {
  x: number
  y: number
  /** The layer the feet are in. */
  z = GROUND
  private speed: number
  private dt: number
  private radius: number
  private pending: [number, number, number][] = []

  constructor(x: number, y: number, speed: number, hz: number, radius: number) {
    this.x = x
    this.y = y
    this.speed = f(speed)
    this.dt = f(1 / hz)
    this.radius = f(radius)
  }

  /** Predict one tick of an input that is also being sent to the server. */
  applyInput(world: World, seq: number, dirX: number, dirY: number) {
    ;[this.x, this.y, this.z] = integrate(world, this.x, this.y, this.z, dirX, dirY, this.speed, this.dt, this.radius)
    if (this.pending.length === MAX_PENDING) this.pending.shift()
    this.pending.push([seq, dirX, dirY])
  }

  /** Adopt the server's position and replay what it has not processed yet. Returns the size of the correction. */
  reconcile(world: World, serverX: number, serverY: number, serverZ: number, lastInputSeq: number): number {
    const px = this.x
    const py = this.y
    // Wrapping compare, so sequence overflow is harmless.
    while (this.pending.length && ((lastInputSeq - this.pending[0]![0]) | 0) >= 0) this.pending.shift()
    this.x = serverX
    this.y = serverY
    this.z = serverZ
    for (const [, dx, dy] of this.pending) [this.x, this.y, this.z] = integrate(world, this.x, this.y, this.z, dx, dy, this.speed, this.dt, this.radius)
    return Math.hypot(this.x - px, this.y - py)
  }

  get unacked() {
    return this.pending.length
  }
}
