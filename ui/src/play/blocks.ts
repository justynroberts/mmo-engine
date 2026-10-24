// Built blocks as /play/blocks and /api/blocks list them: per column its cell, its base layer (the layer its
// ground is in), one character per layer from there ('.' empty, '0'-'9' then 'a'-'z' a material) and an owner
// per layer (0 nobody, otherwise an index into the response's `owners` plus one).

/** Block materials (proto::game::BLOCK_KINDS). */
export const BLOCK_KINDS = 14

/** A material from its character in block text, or -1 for an empty layer or anything else. */
export function kindOf(ch: string): number {
  const k = ch ? parseInt(ch, 36) : NaN
  return Number.isInteger(k) && k >= 0 && k < BLOCK_KINDS && ch !== '.' ? k : -1
}

export interface BlockCol {
  cx: number
  cy: number
  base: number
  kinds: string
  /** Per layer: an index into the old feed's `owners` plus one (0 nobody), or, streamed by region, the tag. */
  owners: (number | string)[]
}

/** The column's layers as a bit mask (bit k = a block in layer base + k), as the collision world takes it. */
export function maskOf(kinds: string): number {
  let m = 0
  for (let k = 0; k < kinds.length && k < 16; k++) if (kinds[k] !== '.') m |= 1 << k
  return m
}

/** Each block in a column: its layer counted from the column's base, and its material. */
export function* blocksOf(c: Pick<BlockCol, 'kinds'>): Generator<[number, number]> {
  for (let k = 0; k < c.kinds.length; k++) {
    const kind = kindOf(c.kinds[k]!)
    if (kind >= 0) yield [k, kind]
  }
}

// ------------------------------------------------------------------ streaming by region (/play/blocks/r)

/** Cells along a region's side (proto::blockfeed::REGION): 256 world units, as a wall tile. */
export const REGION = 128

/** A region as /play/blocks/r sends it: where, its version, and its columns (cleared ones are simply absent). */
export interface BlockRegion {
  rx: number
  ry: number
  ver: number
  cols: { cx: number; cy: number; base: number; kinds: string; owners: string[] }[]
}

const KIND_CHARS = '0123456789abcdefghijklmnopqrstuvwxyz'

/** Read a /play/blocks/r reply (proto::blockfeed): null if it is cut short or not that format. */
export function decodeRegions(buf: ArrayBuffer): BlockRegion[] | null {
  const v = new DataView(buf)
  let p = 0
  const need = (n: number) => p + n <= v.byteLength
  if (!need(3) || v.getUint8(0) !== 1) return null
  const n = v.getUint16(1, true)
  p = 3
  const out: BlockRegion[] = []
  for (let r = 0; r < n; r++) {
    if (!need(10)) return null
    const rx = v.getUint16(p, true)
    const ry = v.getUint16(p + 2, true)
    const ver = v.getUint32(p + 4, true)
    const nOwners = v.getUint16(p + 8, true)
    p += 10
    const owners: string[] = []
    for (let o = 0; o < nOwners; o++) {
      if (!need(8)) return null
      let tag = ''
      for (let i = 0; i < 8; i++) tag += v.getUint8(p + i).toString(16).padStart(2, '0')
      owners.push(tag)
      p += 8
    }
    if (!need(2)) return null
    const nCols = v.getUint16(p, true)
    p += 2
    const cols: BlockRegion['cols'] = []
    for (let c = 0; c < nCols; c++) {
      if (!need(5)) return null
      const local = v.getUint16(p, true)
      const base = v.getInt8(p + 2)
      const mask = v.getUint16(p + 3, true)
      p += 5
      let top = 0
      for (let k = 0; k < 16; k++) if (mask & (1 << k)) top = k + 1
      let kinds = ''
      const own: string[] = []
      for (let k = 0; k < top; k++) {
        if (mask & (1 << k)) {
          if (!need(2)) return null
          const o = v.getUint8(p + 1)
          kinds += KIND_CHARS[v.getUint8(p)] ?? '.'
          own.push(o > 0 ? (owners[o - 1] ?? '') : '')
          p += 2
        } else {
          kinds += '.'
          own.push('')
        }
      }
      cols.push({ cx: rx * REGION + (local % REGION), cy: ry * REGION + Math.floor(local / REGION), base, kinds, owners: own })
    }
    out.push({ rx, ry, ver, cols })
  }
  return out
}

/** The regions within `reach` of the one a point is in, as `c=` asks for them, with the version held of each. */
export function regionAsk(x: number, y: number, block: number, reach: number, held: Map<number, number>): string {
  const span = REGION * block
  const [rx, ry] = [Math.floor(x / span), Math.floor(y / span)]
  const parts: string[] = []
  for (let dy = -reach; dy <= reach; dy++)
    for (let dx = -reach; dx <= reach; dx++) {
      const [qx, qy] = [rx + dx, ry + dy]
      if (qx >= 0 && qy >= 0) parts.push(`${qx},${qy},${held.get((qy << 16) | qx) ?? 0}`)
    }
  return parts.join(';')
}
