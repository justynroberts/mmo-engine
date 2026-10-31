// The walls the browser knows about. Small worlds send them all with the join info; big ones are fetched from
// /play/tiles a tile at a time around the player, and far tiles are dropped again, so a 65,536 unit world costs
// the same memory as a small one. The reply format is proto::tiles (decode it the same way: tiles.test.ts and the
// Rust tests share a fixture).

import { roadKey, type Box } from './sim'
import { playUrl } from './base'

export interface WallsMeta {
  ver: string
  /** Edge of a tile in world units (proto::tiles::TILE). */
  tile: number
  count: number
  inline: boolean
}

/** What a wall is, for drawing (proto::tiles::class). Collision treats them all the same. */
export const WALL = 0
export const SCENERY = 1

/** Road cells along a tile's edge (proto::tiles::ROAD_CELLS). */
const ROAD_CELLS = 64

export interface TileBox {
  id: number
  box: Box
  cls: number
}

/** A /play/tiles reply: its version (16 hex digits), each tile's boxes and each tile's road cells (as roadKeys).
 * Null if cut short or not format 2. */
export function decodeTiles(buf: ArrayBuffer): { ver: string; tiles: Map<string, TileBox[]>; roads: Map<string, number[]>; waters: Map<string, number[]> } | null {
  const v = new DataView(buf)
  let p = 0
  const need = (n: number) => p + n <= v.byteLength
  if (!need(11) || v.getUint8(0) !== 2) return null
  const ver = v.getBigUint64(1, true).toString(16).padStart(16, '0')
  const n = v.getUint16(9, true)
  p = 11
  const tiles = new Map<string, TileBox[]>()
  const roads = new Map<string, number[]>()
  for (let t = 0; t < n; t++) {
    if (!need(8)) return null
    const cx = v.getUint16(p, true)
    const cy = v.getUint16(p + 2, true)
    const k = v.getUint32(p + 4, true)
    p += 8
    if (k > MAX_PER_TILE || !need(k * 21)) return null
    const list: TileBox[] = []
    for (let i = 0; i < k; i++, p += 21) {
      const f = (o: number) => v.getFloat32(p + o, true)
      list.push({ id: v.getUint32(p, true), box: [f(4), f(8), f(12), f(16)], cls: v.getUint8(p + 20) })
    }
    tiles.set(`${cx},${cy}`, list)
    if (!need(2)) return null
    const m = v.getUint16(p, true)
    p += 2
    if (m > ROAD_CELLS * ROAD_CELLS || !need(m * 2)) return null
    const cells: number[] = []
    for (let i = 0; i < m; i++, p += 2) {
      const c = Math.min(v.getUint16(p, true), ROAD_CELLS * ROAD_CELLS - 1)
      cells.push(roadKey(cx * ROAD_CELLS + (c % ROAD_CELLS), cy * ROAD_CELLS + Math.floor(c / ROAD_CELLS)))
    }
    roads.set(`${cx},${cy}`, cells)
  }
  // After the tiles: their deep water cells (proto::tiles; a server too old to have them sends nothing more).
  const waters = new Map<string, number[]>()
  if (need(2)) {
    const w = v.getUint16(p, true)
    p += 2
    for (let t = 0; t < Math.min(w, 64); t++) {
      if (!need(6)) return null
      const cx = v.getUint16(p, true)
      const cy = v.getUint16(p + 2, true)
      const m = v.getUint16(p + 4, true)
      p += 6
      if (m > ROAD_CELLS * ROAD_CELLS || !need(m * 2)) return null
      const cells: number[] = []
      for (let i = 0; i < m; i++, p += 2) {
        const c = Math.min(v.getUint16(p, true), ROAD_CELLS * ROAD_CELLS - 1)
        cells.push(roadKey(cx * ROAD_CELLS + (c % ROAD_CELLS), cy * ROAD_CELLS + Math.floor(c / ROAD_CELLS)))
      }
      waters.set(`${cx},${cy}`, cells)
    }
  }
  return { ver, tiles, roads, waters }
}

type Fetcher = (url: string) => Promise<ArrayBuffer>

/** Tiles kept around the player, in each direction: 2 means a 5x5 window, and 3 is when a tile is let go. */
const NEAR = 2
const KEEP = 3
const MAX_PER_TILE = 4096

const defaultFetch: Fetcher = (url) => fetch(url).then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(String(r.status)))))

export class WallStore {
  /** Bumps whenever the set of boxes changes, so consumers can cheap-check instead of diffing. */
  rev = 0
  private boxes = new Map<number, Box>()
  private cls = new Map<number, number>()
  private refs = new Map<number, number>()
  private tiles = new Map<string, number[]>()
  /** Each held tile's road cells (roadKeys), and its deep water cells. */
  private roads = new Map<string, number[]>()
  private waters = new Map<string, number[]>()
  private pending = new Set<string>()
  private ver = ''
  private streaming = false
  private tileSize = 256
  private cx = NaN
  private cy = NaN
  private epoch = 0

  constructor(
    private worldSize: number,
    meta: WallsMeta | undefined,
    inline: readonly Box[],
    classes: readonly number[] = [],
    private fetcher: Fetcher = defaultFetch,
  ) {
    this.adopt(meta, inline, classes)
  }

  get isStreaming() {
    return this.streaming
  }

  /** Every wall currently held. */
  list(): Box[] {
    return [...this.boxes.values()]
  }

  /** Every wall held with what it is: buildings to draw as blocks, scenery the world file already draws. */
  entries(): { box: Box; cls: number }[] {
    return [...this.boxes].map(([id, box]) => ({ box, cls: this.cls.get(id) ?? WALL }))
  }

  /** Every road cell held (roadKeys). */
  roadCells(): number[] {
    return [...this.roads.values()].flat()
  }

  /** Every deep water cell held (roadKeys). */
  waterCells(): number[] {
    return [...this.waters.values()].flat()
  }

  /** Walls whose box touches the square of half-width `r` around a point. */
  near(x: number, y: number, r: number): Box[] {
    const out: Box[] = []
    for (const b of this.boxes.values()) if (!(b[2] < x - r || b[0] > x + r || b[3] < y - r || b[1] > y + r)) out.push(b)
    return out
  }

  /** Take the join info or a fresh poll of it. Returns true when the walls changed. */
  adopt(meta: WallsMeta | undefined, inline: readonly Box[], classes: readonly number[] = []): boolean {
    if (meta && !meta.inline) {
      if (this.streaming && meta.ver === this.ver) return false
      this.reset()
      this.streaming = true
      this.ver = meta.ver
      this.tileSize = meta.tile ?? 256
      this.cx = this.cy = NaN
      this.rev++
      return true
    }
    const same =
      !this.streaming && this.boxes.size === inline.length && inline.every((b, i) => eq(this.boxes.get(i), b) && (this.cls.get(i) ?? WALL) === (classes[i] ?? WALL))
    if (same) return false
    this.reset()
    this.streaming = false
    inline.forEach((b, i) => {
      this.boxes.set(i, [b[0], b[1], b[2], b[3]])
      this.cls.set(i, classes[i] ?? WALL)
    })
    this.rev++
    return true
  }

  private reset() {
    this.boxes.clear()
    this.cls.clear()
    this.refs.clear()
    this.tiles.clear()
    this.roads.clear()
    this.waters.clear()
    this.pending.clear()
    this.epoch++
  }

  /** Call as the player moves. Asks for what is missing near them and lets go of what is far. */
  update(x: number, y: number) {
    if (!this.streaming) return
    const c = this.tileSize
    const cx = Math.floor(Math.max(0, x) / c)
    const cy = Math.floor(Math.max(0, y) / c)
    if (cx === this.cx && cy === this.cy) return
    this.cx = cx
    this.cy = cy
    const max = Math.max(0, Math.floor((this.worldSize - 1) / c))
    const want: string[] = []
    for (let j = Math.max(0, cy - NEAR); j <= Math.min(max, cy + NEAR); j++) {
      for (let i = Math.max(0, cx - NEAR); i <= Math.min(max, cx + NEAR); i++) {
        const k = `${i},${j}`
        if (!this.tiles.has(k) && !this.pending.has(k)) want.push(k)
      }
    }
    let changed = false
    for (const k of [...this.tiles.keys()]) {
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
    // The version in the URL makes the reply cacheable for good; a 409 means the walls changed under us, and the
    // next poll of /play/info starts us over.
    this.fetcher(playUrl(`/play/tiles?v=${this.ver}&c=${keys.join('_')}`))
      .then((buf) => {
        if (epoch !== this.epoch) return // the world changed under the request
        const reply = decodeTiles(buf)
        if (!reply || reply.ver !== this.ver) {
          for (const k of keys) this.pending.delete(k)
          return
        }
        for (const k of keys) {
          this.pending.delete(k)
          const ids: number[] = []
          for (const { id, box, cls } of reply.tiles.get(k) ?? []) {
            ids.push(id)
            this.refs.set(id, (this.refs.get(id) ?? 0) + 1)
            if (!this.boxes.has(id)) {
              this.boxes.set(id, box)
              this.cls.set(id, cls)
            }
          }
          this.tiles.set(k, ids)
          this.roads.set(k, reply.roads.get(k) ?? [])
          this.waters.set(k, reply.waters.get(k) ?? [])
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
    for (const id of this.tiles.get(k) ?? []) {
      const n = (this.refs.get(id) ?? 1) - 1
      if (n <= 0) {
        this.refs.delete(id)
        this.boxes.delete(id)
        this.cls.delete(id)
      } else this.refs.set(id, n)
    }
    this.tiles.delete(k)
    this.roads.delete(k)
    this.waters.delete(k)
  }

  /** True once every tile near the player is in, which is when prediction can be trusted. */
  get ready() {
    return !this.streaming || (this.pending.size === 0 && this.tiles.size > 0)
  }
}

function eq(a: Box | undefined, b: Box) {
  return !!a && a[0] === b[0] && a[1] === b[1] && a[2] === b[2] && a[3] === b[3]
}
