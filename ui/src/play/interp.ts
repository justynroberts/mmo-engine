// Snapshot interpolation. The server sends 20 snapshots a second and the network delivers them with jitter,
// so drawing the newest one as it arrives makes other characters stutter. Instead the browser draws the world
// a little in the past (two ticks), blending between the two snapshots either side of that moment.

export interface Pos {
  id: number
  x: number
  y: number
  attrs: Record<string, number>
}

interface Frame<T extends Pos> {
  tick: number
  list: T[]
  byId: Map<number, T>
}

const KEEP = 24

export class Interpolator<T extends Pos = Pos> {
  private frames: Frame<T>[] = []
  private offset = 0
  private tickMs: number

  /** `delay` is how many ticks behind the newest snapshot the picture is drawn; `snap` is the jump (in world
   * units) between two snapshots that is treated as a teleport rather than a very fast walk. */
  constructor(hz: number, private delay = 2, private snap = 48) {
    this.tickMs = 1000 / hz
  }

  reset() {
    this.frames = []
    this.offset = 0
  }

  get size() {
    return this.frames.length
  }

  push(tick: number, list: T[], now: number) {
    const last = this.frames[this.frames.length - 1]
    if (last && tick <= last.tick) {
      // A tick going backwards a long way is a server restart; a small step back is a late duplicate.
      if (last.tick - tick > 40) this.reset()
      else return
    }
    // The server's clock in ticks, as seen from here. The least-delayed snapshot is the best estimate of it,
    // so follow the largest sample and let it decay slowly in case the two clocks run at different rates.
    const sample = tick - now / this.tickMs
    this.offset = this.frames.length === 0 ? sample : Math.max(sample, this.offset - 0.02)
    this.frames.push({ tick, list, byId: new Map(list.map((e) => [e.id, e])) })
    if (this.frames.length > KEEP) this.frames.shift()
  }

  /** The tick being drawn at `now`, as a fraction. */
  renderTick(now: number): number {
    return now / this.tickMs + this.offset - this.delay
  }

  sample(now: number): T[] {
    const fr = this.frames
    if (fr.length === 0) return []
    const rt = this.renderTick(now)
    const first = fr[0]!
    const last = fr[fr.length - 1]!
    if (rt <= first.tick) return first.list
    if (rt >= last.tick) return last.list // stalled or caught up: hold the newest, never extrapolate
    let i = fr.length - 2
    while (i > 0 && fr[i]!.tick > rt) i--
    const a = fr[i]!
    const b = fr[i + 1]!
    const t = (rt - a.tick) / (b.tick - a.tick)
    const out: T[] = []
    for (const eb of b.list) {
      const ea = a.byId.get(eb.id)
      if (!ea) {
        out.push(eb)
        continue
      }
      if (Math.hypot(eb.x - ea.x, eb.y - ea.y) > this.snap) {
        out.push(t < 0.5 ? ea : eb)
        continue
      }
      out.push({ ...eb, x: ea.x + (eb.x - ea.x) * t, y: ea.y + (eb.y - ea.y) * t, attrs: t < 0.5 ? ea.attrs : eb.attrs })
    }
    return out
  }
}
