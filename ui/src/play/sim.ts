// The movement step the server runs, so the browser can predict its own character instead of waiting a round
// trip to see a keypress. It mirrors crates/proto/src/world.rs and predict.rs. The server uses f32, so the
// arithmetic is rounded to f32 at each step (Math.fround); where the two still disagree the next snapshot
// corrects it, which is what reconcile() is for.

export type Box = readonly [number, number, number, number]

const f = Math.fround
const CELL = 64
/** Edge of a building block (proto::world::BLOCK). */
export const BLOCK = 2
/** Blocks a walker steps up without help (proto::world::CLIMB); it drops off any height. */
export const CLIMB = 1
const LAST_CELL = (1 << 15) - 1

export class World {
  readonly size: number
  private boxes: Box[]
  private gw: number
  private buckets: Map<number, number[]>
  /** Heights of the built block columns, by cy << 15 | cx; never 0. */
  private blocks = new Map<number, number>()

  constructor(size: number, boxes: readonly Box[]) {
    this.size = f(size)
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

  /** A column's height in blocks; 0 clears it. */
  setBlock(cx: number, cy: number, height: number) {
    if (cx < 0 || cy < 0 || cx > LAST_CELL || cy > LAST_CELL) return
    if (height > 0) this.blocks.set((cy << 15) | cx, Math.min(255, height))
    else this.blocks.delete((cy << 15) | cx)
  }

  blockHeight(cx: number, cy: number): number {
    return this.blocks.get((cy << 15) | cx) ?? 0
  }

  /** How many blocks up a circle at (x, y) stands: the tallest column it overlaps, 0 on the ground. */
  level(x: number, y: number, r: number): number {
    if (this.blocks.size === 0) return 0
    const cx0 = Math.max(0, Math.floor(f(x - r) / BLOCK))
    const cx1 = Math.min(LAST_CELL, Math.floor(f(x + r) / BLOCK))
    const cy0 = Math.max(0, Math.floor(f(y - r) / BLOCK))
    const cy1 = Math.min(LAST_CELL, Math.floor(f(y + r) / BLOCK))
    let top = 0
    for (let cy = cy0; cy <= cy1; cy++) {
      for (let cx = cx0; cx <= cx1; cx++) {
        const h = this.blockHeight(cx, cy)
        if (h <= top) continue
        const bx = cx * BLOCK
        const by = cy * BLOCK
        if (f(x + r) > bx && f(x - r) < bx + BLOCK && f(y + r) > by && f(y - r) < by + BLOCK) top = h
      }
    }
    return top
  }

  /** True if a circle (treated as its bounding square) at (x, y) leaves the world or overlaps a wall or block. */
  blocked(x: number, y: number, r: number): boolean {
    return this.level(x, y, r) > 0 || this.blockedByWalls(x, y, r)
  }

  /** True if something standing `level` blocks up cannot be at (x, y): a wall, or a column above its climb. */
  blockedFrom(x: number, y: number, r: number, level: number): boolean {
    return this.level(x, y, r) > Math.min(255, level + CLIMB) || this.blockedByWalls(x, y, r)
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

  /** Axis-separated move so a character slides along a wall instead of sticking to it, climbing blocks
   *  CLIMB at a time from where the move starts. */
  step(x: number, y: number, dx: number, dy: number, r: number): [number, number] {
    const level = this.level(x, y, r)
    const nx = f(x + dx)
    const x1 = this.blockedFrom(nx, y, r, level) ? x : nx
    const ny = f(y + dy)
    const y1 = this.blockedFrom(x1, ny, r, level) ? y : ny
    return [x1, y1]
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

export function integrate(world: World, x: number, y: number, dirX: number, dirY: number, speed: number, dt: number, r: number): [number, number] {
  const [mx, my] = moveVector(dirX, dirY)
  if (mx === 0 && my === 0) return [x, y]
  return world.step(x, y, f(f(mx * speed) * dt), f(f(my * speed) * dt), r)
}

/** Inputs kept for replay: over three seconds at 20 Hz. */
const MAX_PENDING = 64

export class Predictor {
  x: number
  y: number
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
    ;[this.x, this.y] = integrate(world, this.x, this.y, dirX, dirY, this.speed, this.dt, this.radius)
    if (this.pending.length === MAX_PENDING) this.pending.shift()
    this.pending.push([seq, dirX, dirY])
  }

  /** Adopt the server's position and replay what it has not processed yet. Returns the size of the correction. */
  reconcile(world: World, serverX: number, serverY: number, lastInputSeq: number): number {
    const px = this.x
    const py = this.y
    // Wrapping compare, so sequence overflow is harmless.
    while (this.pending.length && ((lastInputSeq - this.pending[0]![0]) | 0) >= 0) this.pending.shift()
    this.x = serverX
    this.y = serverY
    for (const [, dx, dy] of this.pending) [this.x, this.y] = integrate(world, this.x, this.y, dx, dy, this.speed, this.dt, this.radius)
    return Math.hypot(this.x - px, this.y - py)
  }

  get unacked() {
    return this.pending.length
  }
}
