import { decodeRegions, regionAsk } from './blocks'

/** The same bytes proto::blockfeed's test makes (and Godot reads): two regions, the second empty. */
const FIXTURE = '0102000100020007000000010000aa00bb00cc00dd02008100ff050002010a01ff3f0500800000020000000200000000000000'

function bytes(hex: string): ArrayBuffer {
  const b = new Uint8Array(hex.length / 2)
  for (let i = 0; i < b.length; i++) b[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16)
  return b.buffer
}

describe('blocks by region', () => {
  it('reads what the server sends', () => {
    const r = decodeRegions(bytes(FIXTURE))!
    expect(r.map((g) => [g.rx, g.ry, g.ver, g.cols.length])).toEqual([
      [1, 2, 7, 2],
      [2, 0, 2, 0],
    ])
    expect(r[0]!.cols[0]).toEqual({ cx: 129, cy: 257, base: -1, kinds: '2.a', owners: ['00aa00bb00cc00dd', '', '00aa00bb00cc00dd'] })
    expect(r[0]!.cols[1]).toEqual({ cx: 255, cy: 383, base: 5, kinds: '...............0', owners: Array(16).fill('') })
  })

  it('refuses a reply cut short', () => {
    expect(decodeRegions(bytes(FIXTURE.slice(0, -6)))).toBeNull()
    expect(decodeRegions(bytes('02'))).toBeNull()
  })

  it('asks for the square round us with the versions held', () => {
    const held = new Map([[(1 << 16) | 1, 42]])
    // 300 units in with 2-unit blocks is region (1, 1); reach 1 is the 3 x 3 round it.
    expect(regionAsk(300, 300, 2, 1, held)).toBe('0,0,0;1,0,0;2,0,0;0,1,0;1,1,42;2,1,0;0,2,0;1,2,0;2,2,0')
    expect(regionAsk(10, 10, 2, 1, held).split(';')).toHaveLength(4)
  })
})
