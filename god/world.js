// MIT License - Copyright (c) fintonlabs.com
// The world file is 17 MB of terrain; the desk needs a picture of it. This cuts it down to what a map draws:
// a coarse grid of heights (0 is sea), the roads, the rivers and the settlements.
'use strict'

const GRID = 768 // cells along each side of the picture

/** Reduce a parsed world file to { size, grid, land (Uint8Array, 0 sea, 1..255 height), roads, rivers, towns }. */
function reduceWorld(wf) {
  const size = Number(wf.size) || 0
  const out = { size, grid: 0, land: new Uint8Array(0), roads: [], rivers: [], towns: [] }
  const h = wf.heights
  if (h && typeof h.u16le === 'string' && h.w > 1 && h.h > 1 && size > 0) {
    const raw = Buffer.from(h.u16le, 'base64')
    const sea = Number(wf.water && wf.water.sea && wf.water.sea.level) || 0
    const span = h.hi - h.lo || 1
    const land = new Uint8Array(GRID * GRID)
    for (let gy = 0; gy < GRID; gy++) {
      const wy = ((gy + 0.5) / GRID) * size
      const sy = Math.min(h.h - 1, Math.max(0, Math.round((wy - h.y0) / h.cell)))
      for (let gx = 0; gx < GRID; gx++) {
        const wx = ((gx + 0.5) / GRID) * size
        const sx = Math.min(h.w - 1, Math.max(0, Math.round((wx - h.x0) / h.cell)))
        const at = (sy * h.w + sx) * 2
        if (at + 1 >= raw.length) continue
        const metres = h.lo + (raw.readUInt16LE(at) / 65535) * span
        land[gy * GRID + gx] = metres <= sea ? 0 : 1 + Math.min(254, Math.round(((metres - sea) / Math.max(h.hi - sea, 1)) * 254))
      }
    }
    out.grid = GRID
    out.land = land
  }
  const line = (pts) => pts.flatMap((p) => [Math.round(p[0]), Math.round(p[1])])
  out.roads = (wf.roads || []).filter((r) => Array.isArray(r.pts) && r.pts.length > 1).map((r) => line(r.pts))
  out.rivers = ((wf.water && wf.water.rivers) || []).filter((r) => Array.isArray(r.pts) && r.pts.length > 1).map((r) => line(r.pts.filter((_, i) => i % 6 === 0 || i === r.pts.length - 1)))
  out.towns = (wf.settled || []).filter((s) => Array.isArray(s.at)).map((s) => ({ name: String(s.name || ''), x: s.at[0], y: s.at[1], t: String(s.t || '') }))
  return out
}

module.exports = { reduceWorld, GRID }
