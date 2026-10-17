// The walls the browser knows about. Small worlds send them all with the join info; big ones are fetched from
// /play/walls a chunk at a time around the player, and far chunks are dropped again, so a 65,536 unit world costs
// the same memory as a small one.

import type { Box } from './sim'

export interface WallsMeta {
  ver: string
  chunk: number
  count: number
  inline: boolean
}

type Fetcher = (url: string) => Promise<unknown>
interface ChunkReply {
  ver: string
  chunks: Record<string, [number, number, number, number, number][]>
  all?: [number, number, number, number, number][]
}

/** Chunks kept around the player, in each direction: 2 means a 5x5 window, and 3 is when a chunk is let go. */
const NEAR = 2
const KEEP = 3

const defaultFetch: Fetcher = (url) => fetch(url).then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))

export class WallStore {
  /** Bumps whenever the set of boxes changes, so consumers can cheap-check instead of diffing. */
  rev = 0
  private boxes = new Map<number, Box>()
  private refs = new Map<number, number>()
  private chunks = new Map<string, number[]>()
  private pending = new Set<string>()
  private ver = ''
  private streaming = false
  private chunkSize = 256
  private cx = NaN
  private cy = NaN
  private epoch = 0

  constructor(
    private worldSize: number,
    meta: WallsMeta | undefined,
    inline: readonly Box[],
    private fetcher: Fetcher = defaultFetch,
  ) {
    this.adopt(meta, inline)
  }

  get isStreaming() {
    return this.streaming
  }

  /** Every wall currently held. */
  list(): Box[] {
    return [...this.boxes.values()]
  }

  /** Walls whose box touches the square of half-width `r` around a point. */
  near(x: number, y: number, r: number): Box[] {
    const out: Box[] = []
    for (const b of this.boxes.values()) if (!(b[2] < x - r || b[0] > x + r || b[3] < y - r || b[1] > y + r)) out.push(b)
    return out
  }

  /** Take the join info or a fresh poll of it. Returns true when the walls changed. */
  adopt(meta: WallsMeta | undefined, inline: readonly Box[]): boolean {
    if (meta && !meta.inline) {
      if (this.streaming && meta.ver === this.ver) return false
      this.reset()
      this.streaming = true
      this.ver = meta.ver
      this.chunkSize = meta.chunk
      this.cx = this.cy = NaN
      this.rev++
      return true
    }
    const same = !this.streaming && this.boxes.size === inline.length && inline.every((b, i) => eq(this.boxes.get(i), b))
    if (same) return false
    this.reset()
    this.streaming = false
    inline.forEach((b, i) => this.boxes.set(i, [b[0], b[1], b[2], b[3]]))
    this.rev++
    return true
  }

  private reset() {
    this.boxes.clear()
    this.refs.clear()
    this.chunks.clear()
    this.pending.clear()
    this.epoch++
  }

  /** Call as the player moves. Asks for what is missing near them and lets go of what is far. */
  update(x: number, y: number) {
    if (!this.streaming) return
    const c = this.chunkSize
    const cx = Math.floor(x / c)
    const cy = Math.floor(y / c)
    if (cx === this.cx && cy === this.cy) return
    this.cx = cx
    this.cy = cy
    const max = Math.max(0, Math.floor((this.worldSize - 1) / c))
    const want: string[] = []
    for (let j = Math.max(0, cy - NEAR); j <= Math.min(max, cy + NEAR); j++) {
      for (let i = Math.max(0, cx - NEAR); i <= Math.min(max, cx + NEAR); i++) {
        const k = `${i},${j}`
        if (!this.chunks.has(k) && !this.pending.has(k)) want.push(k)
      }
    }
    let changed = false
    for (const k of [...this.chunks.keys()]) {
      const [i, j] = k.split(',').map(Number) as [number, number]
      if (Math.abs(i - cx) > KEEP || Math.abs(j - cy) > KEEP) {
        this.drop(k)
        changed = true
      }
    }
    if (changed) this.rev++
    if (want.length) this.request(want)
  }

  private request(keys: string[]) {
    const epoch = this.epoch
    for (const k of keys) this.pending.add(k)
    this.fetcher(`/play/walls?c=${keys.join(';')}`)
      .then((r) => {
        if (epoch !== this.epoch) return // the world changed under the request
        const reply = r as ChunkReply
        if (reply.ver !== this.ver) {
          // Edited since we last looked: the next poll will reset us, and this reply is for walls we no longer want.
          for (const k of keys) this.pending.delete(k)
          return
        }
        for (const k of keys) {
          this.pending.delete(k)
          const ids: number[] = []
          for (const [id, x0, y0, x1, y1] of reply.chunks[k] ?? []) {
            ids.push(id)
            this.refs.set(id, (this.refs.get(id) ?? 0) + 1)
            if (!this.boxes.has(id)) this.boxes.set(id, [x0, y0, x1, y1])
          }
          this.chunks.set(k, ids)
        }
        this.rev++
      })
      .catch(() => {
        if (epoch !== this.epoch) return
        for (const k of keys) this.pending.delete(k)
        this.cx = this.cy = NaN // try again on the next move
      })
  }

  private drop(k: string) {
    for (const id of this.chunks.get(k) ?? []) {
      const n = (this.refs.get(id) ?? 1) - 1
      if (n <= 0) {
        this.refs.delete(id)
        this.boxes.delete(id)
      } else this.refs.set(id, n)
    }
    this.chunks.delete(k)
  }

  /** True once every chunk near the player is in, which is when prediction can be trusted. */
  get ready() {
    return !this.streaming || (this.pending.size === 0 && this.chunks.size > 0)
  }
}

function eq(a: Box | undefined, b: Box) {
  return !!a && a[0] === b[0] && a[1] === b[1] && a[2] === b[2] && a[3] === b[3]
}
