import { describe, expect, it } from 'vitest'
import { decodeCrowd, kind } from './protocol'

describe('decodeCrowd', () => {
  // Byte for byte what proto::wire::encode_crowd writes: part 1 of 2, two entities.
  const bytes = Uint8Array.of(
    kind.CROWD, 1, 2, 2,
    0x39, 0x30, 0, 0, 0x10, 0x00, 0xff, 0xff, 0x82, // id 12345 at (16, 65535), faction 2, a player
    0x07, 0, 0, 0, 0x00, 0x02, 0x03, 0x00, 0x00, //   id 7 at (512, 3), no faction, a monster
  )

  it('reads parts, positions, sides and players', () => {
    expect(decodeCrowd(bytes)).toEqual({
      part: 1,
      parts: 2,
      list: [
        { id: 12345, x: 16.5, y: 65535.5, side: 2, player: true },
        { id: 7, x: 512.5, y: 3.5, side: 0, player: false },
      ],
    })
  })

  it('refuses a short packet or a part past the parts', () => {
    expect(decodeCrowd(bytes.slice(0, bytes.length - 1))).toBeNull()
    const bad = bytes.slice()
    bad[1] = 2
    expect(decodeCrowd(bad)).toBeNull()
  })
})
