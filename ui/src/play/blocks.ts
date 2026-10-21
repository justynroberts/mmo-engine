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
  owners: number[]
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
