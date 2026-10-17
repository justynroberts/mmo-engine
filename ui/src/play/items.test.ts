import { describe, expect, it } from 'vitest'
import { decodeGround, decodeInventory, kind } from './protocol'

describe('item messages', () => {
  it('reads a bag, with 0 meaning an empty slot', () => {
    const b = Uint8Array.of(kind.INVENTORY, 3, 1, 3, 0, 0, 2, 99)
    expect(decodeInventory(b)).toEqual([
      { item: 0, count: 3 },
      { item: -1, count: 0 },
      { item: 1, count: 99 },
    ])
  })

  it('refuses a bag of the wrong length or kind', () => {
    expect(decodeInventory(Uint8Array.of(kind.INVENTORY, 2, 1, 1))).toBeNull()
    expect(decodeInventory(Uint8Array.of(kind.GROUND, 0))).toBeNull()
  })

  it('reads ground items little-endian with positions in world units', () => {
    // id 70000 = 0x00011170, x q=1280 (0x0500), y q=65535, item 2, count 5; POS_SCALE comes from the shared constants.
    const b = Uint8Array.of(kind.GROUND, 1, 0x70, 0x11, 0x01, 0x00, 0x00, 0x05, 0xff, 0xff, 2, 5)
    const g = decodeGround(b)!
    expect(g).toHaveLength(1)
    expect(g[0]).toMatchObject({ id: 70000, item: 2, count: 5 })
    expect(g[0]!.x).toBeGreaterThan(0)
    expect(g[0]!.y).toBeGreaterThan(g[0]!.x)
    expect(decodeGround(Uint8Array.of(kind.GROUND, 0))).toEqual([])
    expect(decodeGround(b.slice(0, 11))).toBeNull()
  })
})
