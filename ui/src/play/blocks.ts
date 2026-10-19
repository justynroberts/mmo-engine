// Built blocks as /play/blocks and /api/blocks list them: per column its cell, its base layer (the layer its
// ground is in), one character per layer from there ('.' empty, '0'-'3' a material) and an owner per layer
// (0 nobody, otherwise an index into the response's `owners` plus one).

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
    const ch = c.kinds.charCodeAt(k) - 48
    if (ch >= 0 && ch < 4) yield [k, ch]
  }
}
