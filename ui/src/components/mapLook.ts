// The map's look, shared by the 2D map and the 3D view: the same meadow, stone and forest as the Godot client.
// The console around the map keeps its own instrument-panel styling; this palette is the world's, not the page's,
// so it does not change with the page theme.

export const MAP = {
  void: '#23361c', // beyond the world's edge: dark forest
  stone: '#8d867d', // building roofs
  stoneEdge: '#3d372f',
  tree: '#2f5a22',
  treeEdge: '#1f3d17',
  trunk: '#7a5232',
  ink: '#1a1a12', // outlines that keep markers readable on grass
  parchment: '#f3ead2', // players and labels
  gold: '#ffd27a', // selection, as the Godot client's target ring
  hpHigh: '#5ac85a',
  hpLow: '#d9412e',
  sky: '#a9c3d6',
  fog: '#a9bfc9',
} as const

// ------------------------------------------------------------------------------------------------ noise

function hash(x: number, y: number, seed: number): number {
  // Integer hash of a lattice point; 32 bit maths so it agrees on every browser.
  let h = Math.imul(x | 0, 0x27d4eb2d) ^ Math.imul(y | 0, 0x165667b1) ^ Math.imul(seed | 0, 0x9e3779b1)
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b)
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35)
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296
}

function noise(x: number, y: number, seed: number): number {
  const ix = Math.floor(x)
  const iy = Math.floor(y)
  const fx = x - ix
  const fy = y - iy
  const ux = fx * fx * (3 - 2 * fx)
  const uy = fy * fy * (3 - 2 * fy)
  const a = hash(ix, iy, seed)
  const b = hash(ix + 1, iy, seed)
  const c = hash(ix, iy + 1, seed)
  const d = hash(ix + 1, iy + 1, seed)
  return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

export function fbm(x: number, y: number, seed: number, octaves = 4): number {
  let v = 0
  let amp = 0.5
  for (let k = 0; k < octaves; k++) {
    v += amp * noise(x, y, seed + k * 101)
    x *= 2.03
    y *= 2.03
    amp *= 0.5
  }
  return v
}

const smooth = (a: number, b: number, t: number) => {
  const k = Math.min(1, Math.max(0, (t - a) / (b - a)))
  return k * k * (3 - 2 * k)
}

/** Ground colour at a world point as sRGB 0..1: two greens, dry patches and worn dirt, as the client's shader. */
export function meadow(x: number, y: number, seed: number, detail = 14): [number, number, number] {
  const m = smooth(0.3, 0.7, fbm(x / detail, y / detail, seed, 3))
  let r = 0.36 + 0.14 * m
  let g = 0.56 + 0.1 * m
  let b = 0.2 + 0.05 * m
  const dry = smooth(0.55, 0.75, fbm(x / 90, y / 90, seed + 7, 3)) * 0.5
  r += (0.46 - r) * dry
  g += (0.43 - g) * dry
  b += (0.24 - b) * dry
  const dirt = smooth(0.62, 0.78, fbm(x / 18 + 7, y / 18 + 7, seed + 13, 3)) * 0.45
  r += (0.44 - r) * dirt
  g += (0.36 - g) * dirt
  b += (0.23 - b) * dirt
  return [r, g, b]
}

type Box = [number, number, number, number]

/** Rolling hills from the seed, levelled around buildings so they stand on flat ground. Visual only. */
export class Terrain {
  private base: number[]
  /** Buildings by 128 unit cell (padded), so a height lookup only checks the few nearby. */
  private cells = new Map<number, number[]>()
  constructor(
    readonly seed: number,
    readonly boxes: Box[],
    readonly amplitude = 15,
    readonly pad = 10,
  ) {
    this.base = boxes.map((b) => this.raw((b[0] + b[2]) / 2, (b[1] + b[3]) / 2))
    boxes.forEach((b, i) => {
      for (let cx = Math.floor((b[0] - pad) / 128); cx <= Math.floor((b[2] + pad) / 128); cx++)
        for (let cy = Math.floor((b[1] - pad) / 128); cy <= Math.floor((b[3] + pad) / 128); cy++) {
          const k = cx * 100003 + cy
          const l = this.cells.get(k)
          if (l) l.push(i)
          else this.cells.set(k, [i])
        }
    })
  }
  raw(x: number, y: number): number {
    return (fbm(x / 170, y / 170, this.seed + 31, 3) - 0.5) * 2 * this.amplitude
  }
  height(x: number, y: number): number {
    const h = this.raw(x, y)
    let best = 0
    let base = h
    for (const i of this.cells.get(Math.floor(x / 128) * 100003 + Math.floor(y / 128)) ?? []) {
      const b = this.boxes[i]!
      const dx = Math.max(b[0] - x, x - b[2], 0)
      const dy = Math.max(b[1] - y, y - b[3], 0)
      if (dx > this.pad || dy > this.pad) continue
      const w = 1 - smooth(0, this.pad, Math.hypot(dx, dy))
      if (w > best) {
        best = w
        base = this.base[i]!
      }
    }
    return h + (base - h) * best
  }
  baseOf(i: number): number {
    return this.base[i] ?? 0
  }
}

/** Trees scattered from the seed, kept out of buildings: [x, y, size]. The same set for both views. */
export function trees(seed: number, size: number, boxes: Box[], cap = 4000): [number, number, number][] {
  const out: [number, number, number][] = []
  const chunk = 128
  const per = 12
  const n = Math.ceil(size / chunk)
  // Fewer per chunk in very large worlds so the total stays under the cap.
  const keep = Math.min(1, cap / (n * n * per))
  for (let cx = 0; cx < n; cx++) {
    for (let cy = 0; cy < n; cy++) {
      for (let k = 0; k < per; k++) {
        if (hash(cx * 31 + k, cy, seed + 5) > keep) continue
        const x = (cx + hash(cx, cy * 977 + k, seed + 1)) * chunk
        const y = (cy + hash(cx * 613 + k, cy, seed + 2)) * chunk
        const s = 2.2 + hash(cx + k, cy + k * 7, seed + 3) * 1.6
        if (x > size || y > size) continue
        if (boxes.some((b) => x > b[0] - 6 && x < b[2] + 6 && y > b[1] - 6 && y < b[3] + 6)) continue
        out.push([x, y, s])
      }
    }
  }
  return out
}

/** A top-down picture of the ground for the 2D map, with hill shading, sized to fit `px` pixels. */
export function groundImage(seed: number, size: number, boxes: Box[], px = 512): HTMLCanvasElement | null {
  const cv = document.createElement('canvas')
  cv.width = cv.height = px
  const ctx = cv.getContext('2d')
  if (!ctx) return null
  const img = ctx.createImageData(px, px)
  const t = new Terrain(seed, boxes)
  const step = size / px
  for (let j = 0; j < px; j++) {
    for (let i = 0; i < px; i++) {
      const x = (i + 0.5) * step
      const y = (j + 0.5) * step
      // Variation finer than a few pixels only shimmers, so the map uses a broader pattern than the ground does.
      const [r, g, b] = meadow(x, y, seed, Math.max(14, step * 6))
      // Light from the north west: brighter on slopes facing it, darker away.
      const e = Math.max(step, 2)
      const slope = (t.raw(x - e, y - e) - t.raw(x + e, y + e)) / (2 * e)
      const lit = 0.9 * (1 + Math.max(-0.25, Math.min(0.25, slope * 1.2)))
      const o = (j * px + i) * 4
      img.data[o] = Math.min(255, r * lit * 255)
      img.data[o + 1] = Math.min(255, g * lit * 255)
      img.data[o + 2] = Math.min(255, b * lit * 255)
      img.data[o + 3] = 255
    }
  }
  ctx.putImageData(img, 0, 0)
  return cv
}

export function hpColor(hp: number): string {
  return hp > 0.5 ? MAP.hpHigh : hp > 0.25 ? '#e0b53a' : MAP.hpLow
}

// ------------------------------------------------------------------------------------------------ land features
// The same box kinds and river courses as the Godot client (godot/scripts/terrain.gd), step for step.

export const KEEP = 0
export const CRAG = 1
export const LAKE = 2

/** Keep, crag or lake, from the box's own corner. */
export function boxKind(b: Box): number {
  const w = b[2] - b[0]
  const d = b[3] - b[1]
  if (Math.min(w, d) < 10) return KEEP
  const h = (Math.trunc(b[0]) * 7 + Math.trunc(b[1]) * 13) % 10
  if (h >= 8 && Math.min(w, d) >= 12) return LAKE
  if (h >= 5) return CRAG
  return KEEP
}

export const RIVER_HALF = 3.5

/**
 * River courses: [x, y] points every 10 units. An LCG on exact integers drives them, and positions are rounded to
 * 32 bit floats each step as Godot's Vector2 is, so both clients trace the same lines.
 */
export function rivers(seed: number, size: number, boxes: Box[]): [number, number][][] {
  const f = Math.fround
  let s = (seed * 2654435761 + 12345) % 4294967296
  const next = () => {
    s = (s * 1664525 + 1013904223) % 4294967296
    return s / 4294967296
  }
  const blocked = (x: number, y: number) => boxes.some((b) => x > b[0] - 14 && x < b[2] + 14 && y > b[1] - 14 && y < b[3] + 14)
  const out: [number, number][][] = []
  const count = Math.min(6, Math.max(2, 2 + Math.trunc(size / 2000)))
  const step = 10
  for (let r = 0; r < count; r++) {
    const side = Math.trunc(next() * 4)
    const along = (0.15 + 0.7 * next()) * size
    const starts: [number, number][] = [[along, 0], [size, along], [along, size], [0, along]]
    const aims = [Math.PI / 2, Math.PI, -Math.PI / 2, 0]
    let p: [number, number] = [f(starts[side]![0]), f(starts[side]![1])]
    const aim = aims[side]!
    let heading = aim + (next() - 0.5) * 0.8
    const step2 = (h: number): [number, number] => [f(p[0] + f(f(Math.cos(h)) * step)), f(p[1] + f(f(Math.sin(h)) * step))]
    const pts: [number, number][] = [p]
    for (let n = 0; n < 2000; n++) {
      heading += (next() - 0.5) * 0.35
      let off = ((aim - heading + Math.PI) % (Math.PI * 2)) - Math.PI
      if (off < -Math.PI) off += Math.PI * 2
      heading += off * 0.06
      let q = step2(heading)
      if (blocked(q[0], q[1])) {
        const course = heading
        for (let t = 0; t < 8; t++) {
          heading = course + 0.4 * (Math.trunc(t / 2) + 1) * (t % 2 === 0 ? 1 : -1)
          q = step2(heading)
          if (!blocked(q[0], q[1])) break
        }
      }
      const outside = q[0] < 0 || q[1] < 0 || q[0] > size || q[1] > size
      if (outside && n < 30) {
        heading = aim
        continue
      }
      if (outside) {
        pts.push(q)
        break
      }
      p = q
      pts.push(p)
    }
    out.push(pts)
  }
  return out
}

export const WATER = '#2f7f8f'
export const WATER_DEEP = '#1d4f66'
export const SAND = '#c9b68a'
export const ROCK = '#7d776e'
