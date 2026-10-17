// The movement step the server runs, so the browser can predict its own character instead of waiting a round
// trip to see a keypress. It mirrors crates/proto/src/world.rs and predict.rs. The server uses f32, so the
// arithmetic is rounded to f32 at each step (Math.fround); where the two still disagree the next snapshot
// corrects it, which is what reconcile() is for.

export type Box = readonly [number, number, number, number]

const f = Math.fround
const CELL = 64

export class World {
  readonly size: number
  private boxes: Box[]
  private gw: number
  private buckets: Map<number, number[]>

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

  /** True if a circle (treated as its bounding square) at (x, y) leaves the world or overlaps a wall. */
  blocked(x: number, y: number, r: number): boolean {
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

  /** Axis-separated move so a character slides along a wall instead of sticking to it. */
  step(x: number, y: number, dx: number, dy: number, r: number): [number, number] {
    const nx = f(x + dx)
    const x1 = this.blocked(nx, y, r) ? x : nx
    const ny = f(y + dy)
    const y1 = this.blocked(x1, ny, r) ? y : ny
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
