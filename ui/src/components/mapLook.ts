// How the console draws the world on its maps: a palette by object type, and a picture of the ground shaded from
// the world file's heights. It decides nothing about where anything is (that is the world file, WORLD.md); this
// is only the console's own way of showing it, and does not follow the page theme.

import type { WorldData } from '../world'

export const MAP = {
  void: '#23361c', // beyond the world's edge
  stone: '#8d867d', // a plain wall block
  stoneEdge: '#3d372f',
  tree: '#2f5a22',
  treeEdge: '#1f3d17',
  trunk: '#7a5232',
  ink: '#1a1a12', // outlines that keep markers readable on any ground
  parchment: '#f3ead2', // players and labels
  gold: '#ffd27a', // selection
  hpHigh: '#5ac85a',
  hpLow: '#d9412e',
  sky: '#a9c3d6',
  fog: '#a9bfc9',
  water: '#2f7f8f',
} as const

/** Fill colour of a footprint by its type; an unknown type gets the plain stone. */
export const TYPE_COLOUR: Record<string, string> = {
  house: '#b5523b',
  keep: '#8d867d',
  castle: '#8d867d',
  tower: '#6f6a62',
  wall: '#5d5850',
  crag: '#6e675e',
  lake: MAP.water,
  square: '#b8a77e',
  city: '#9b8f72',
  block: MAP.stone,
}

/** Footprints that are open ground, drawn under everything else. */
export const OPEN = new Set(['city', 'square'])

/** Point objects drawn as tree canopies. */
export const TREES = new Set(['tree', 'tree_bare', 'bush'])

/** Ground colour by height as sRGB 0..1: meadow, rock higher up, snow on the peaks. */
export function groundColour(h: number): [number, number, number] {
  const k = (a: number, b: number, t: number) => Math.min(1, Math.max(0, (t - a) / (b - a)))
  const rock = k(14, 28, h)
  const snow = k(34, 46, h)
  const c: [number, number, number] = [0.36 + (0.47 - 0.36) * rock, 0.54 + (0.45 - 0.54) * rock, 0.2 + (0.42 - 0.2) * rock]
  return [c[0] + (0.93 - c[0]) * snow, c[1] + (0.95 - c[1]) * snow, c[2] + (0.98 - c[2]) * snow]
}

/** A top-down picture of the ground for the 2D map, shaded from the north west, sized to fit `px` pixels. */
export function groundImage(w: WorldData, px = 512): HTMLCanvasElement | null {
  const cv = document.createElement('canvas')
  cv.width = cv.height = px
  const ctx = cv.getContext('2d')
  if (!ctx) return null
  const img = ctx.createImageData(px, px)
  const step = w.size / px
  const e = Math.max(step, 2)
  for (let j = 0; j < px; j++) {
    for (let i = 0; i < px; i++) {
      const x = (i + 0.5) * step
      const y = (j + 0.5) * step
      const slope = (w.height(x - e, y - e) - w.height(x + e, y + e)) / (2 * e)
      const lit = 0.9 * (1 + Math.max(-0.25, Math.min(0.25, slope * 1.2)))
      const [r, g, b] = groundColour(w.height(x, y))
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
