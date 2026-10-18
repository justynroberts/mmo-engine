// The world file (WORLD.md) as the console reads it: heights, water and typed objects. The console draws from
// this, as the game clients do, so nothing here or in the map views generates or decides anything about the land.

export interface WorldObject {
  t: string
  /** Footprint [x0, y0, x1, y1]. */
  b?: [number, number, number, number]
  /** Position of a point object. */
  p?: [number, number]
  /** Ground height a footprint stands on. */
  y?: number
  v?: number
  s?: number
  r?: number
  solid?: boolean
}

export interface River {
  half_width: number
  /** [x, y, water level] */
  pts: [number, number, number][]
}

export interface Lake {
  b: [number, number, number, number]
  level: number
}

/** A hand edit on top of what the seed lays out (worldgen applies them in order). */
export type WorldEdit = { remove: [number, number] } | { add: { t: string; b: [number, number, number, number] } }

/** What a world is made from: worldgen's recipe, carried in the file it made. */
export interface Recipe {
  size: number
  seed: number
  city: boolean
  villages: number
  forts: number
  crags: number
  lakes: number
  /** River count, or null for the usual number for the size. */
  rivers: number | null
  forest: number
  hills: number
  edits: WorldEdit[]
}

export function defaultRecipe(size: number, seed: number): Recipe {
  return { size, seed, city: true, villages: 1, forts: 1, crags: 1, lakes: 1, rivers: null, forest: 1, hills: 1, edits: [] }
}

export interface WorldData {
  size: number
  /** The recipe the file was made from, when its generator wrote one. */
  recipe?: Recipe
  objects: WorldObject[]
  rivers: River[]
  lakes: Lake[]
  /** Ground height at a point (0 without heights). */
  height(x: number, y: number): number
  hasHeights: boolean
}

interface FileHeights {
  x0: number
  y0: number
  cell: number
  w: number
  h: number
  lo: number
  hi: number
  u16le: string
}

/** Read a world file's text; null if it is not one. */
export function parseWorld(text: string): WorldData | null {
  let d: { format?: number; size?: number; recipe?: Partial<Recipe>; heights?: FileHeights; water?: { rivers?: River[]; lakes?: Lake[] }; objects?: WorldObject[] }
  try {
    d = JSON.parse(text)
  } catch {
    return null
  }
  if (d.format !== 1 || typeof d.size !== 'number') return null
  const h = d.heights
  let heights: Float32Array | null = null
  if (h && typeof h.u16le === 'string') {
    const raw = atob(h.u16le)
    heights = new Float32Array(h.w * h.h)
    const span = h.hi - h.lo
    for (let i = 0; i < heights.length; i++) heights[i] = h.lo + (span * (raw.charCodeAt(i * 2) | (raw.charCodeAt(i * 2 + 1) << 8))) / 65535
  }
  const height = (x: number, y: number) => {
    if (!heights || !h) return 0
    const fx = Math.min(Math.max((x - h.x0) / h.cell, 0), h.w - 1)
    const fy = Math.min(Math.max((y - h.y0) / h.cell, 0), h.h - 1)
    const i = Math.min(Math.floor(fx), h.w - 2)
    const j = Math.min(Math.floor(fy), h.h - 2)
    const tx = fx - i
    const ty = fy - j
    const a = heights[j * h.w + i]!
    const b = heights[j * h.w + i + 1]!
    const c = heights[(j + 1) * h.w + i]!
    const e = heights[(j + 1) * h.w + i + 1]!
    return (a + (b - a) * tx) * (1 - ty) + (c + (e - c) * tx) * ty
  }
  const recipe = d.recipe ? { ...defaultRecipe(d.size, 1), ...d.recipe, edits: d.recipe.edits ?? [] } : undefined
  return { size: d.size, recipe, objects: d.objects ?? [], rivers: d.water?.rivers ?? [], lakes: d.water?.lakes ?? [], height, hasHeights: heights !== null }
}

/** A world with no file: the server's walls as plain solid blocks on flat ground. */
export function fromBoxes(size: number, boxes: [number, number, number, number][]): WorldData {
  return {
    size,
    objects: boxes.map((b) => ({ t: 'block', b, solid: true, y: 0 })),
    rivers: [],
    lakes: [],
    height: () => 0,
    hasHeights: false,
  }
}
