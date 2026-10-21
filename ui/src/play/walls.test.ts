import { describe, expect, it } from 'vitest'
import { decodeTiles, SCENERY, WALL, WallStore } from './walls'

/** proto::tiles::tests::FIXTURE: tiles (0,0), (1,0) and (3,3) of two boxes, the second scenery on a tile border. */
const FIXTURE =
  '015d68e04ca311b33f0300000000000200000000000000000028410000a2410000f04100002042000100000000007a43cdcccc3d000096430000a0400101000000010000000100000000007a43cdcccc3d000096430000a040010300030000000000'

const bytes = (hex: string) => new Uint8Array(hex.match(/../g)!.map((h) => parseInt(h, 16))).buffer

const meta = { ver: '00000000000000aa', tile: 256, count: 3, inline: false }
const flush = () => new Promise((r) => setTimeout(r, 0))

/** A /play/tiles reply, the way proto::tiles::TileSet::encode writes one. */
function encode(ver: bigint, tiles: [string, [number, number, number, number, number][]][]): ArrayBuffer {
  const n = tiles.reduce((a, [, l]) => a + 8 + l.length * 21, 11)
  const v = new DataView(new ArrayBuffer(n))
  v.setUint8(0, 1)
  v.setBigUint64(1, ver, true)
  v.setUint16(9, tiles.length, true)
  let p = 11
  for (const [k, list] of tiles) {
    const [cx, cy] = k.split(',').map(Number) as [number, number]
    v.setUint16(p, cx, true)
    v.setUint16(p + 2, cy, true)
    v.setUint32(p + 4, list.length, true)
    p += 8
    for (const [id, x0, y0, x1, y1] of list) {
      v.setUint32(p, id, true)
      ;[x0, y0, x1, y1].forEach((f, i) => v.setFloat32(p + 4 + 4 * i, f, true))
      v.setUint8(p + 20, WALL)
      p += 21
    }
  }
  return v.buffer
}

function server(boxes: [number, number, number, number][]) {
  const asked: string[] = []
  const fetcher = async (url: string) => {
    asked.push(url)
    const q = new URL(url, 'http://x').searchParams
    expect(q.get('v')).toBe(meta.ver)
    const tiles = q.get('c')!.split(';').map((k): [string, [number, number, number, number, number][]] => {
      const [cx, cy] = k.split(',').map(Number) as [number, number]
      const hit = boxes.map((b, i) => [i, ...b] as [number, number, number, number, number])
      return [k, hit.filter((b) => b[3] >= cx * 256 && b[1] < (cx + 1) * 256 && b[4] >= cy * 256 && b[2] < (cy + 1) * 256)]
    })
    return encode(0xaan, tiles)
  }
  return { asked, fetcher }
}

describe('decodeTiles', () => {
  it('reads what the server writes, bit for bit', () => {
    const r = decodeTiles(bytes(FIXTURE))!
    expect(r.ver).toBe('3fb311a34ce0685d')
    expect([...r.tiles.keys()]).toEqual(['0,0', '1,0', '3,3'])
    expect(r.tiles.get('0,0')).toEqual([
      { id: 0, box: [10.5, 20.25, 30, 40], cls: WALL },
      { id: 1, box: [250, Math.fround(0.1), 300, 5], cls: SCENERY },
    ])
    expect(r.tiles.get('1,0')!.map((b) => b.id)).toEqual([1])
    expect(r.tiles.get('3,3')).toEqual([])
    expect(decodeTiles(bytes(FIXTURE).slice(0, 30))).toBeNull()
  })
})

describe('WallStore', () => {
  it('holds inline walls with their classes and notices when they change', () => {
    const s = new WallStore(1000, { ver: 'x', tile: 256, count: 1, inline: true }, [[1, 2, 3, 4]], [SCENERY])
    expect(s.list()).toEqual([[1, 2, 3, 4]])
    expect(s.entries()).toEqual([{ box: [1, 2, 3, 4], cls: SCENERY }])
    expect(s.isStreaming).toBe(false)
    expect(s.adopt(undefined, [[1, 2, 3, 4]], [SCENERY])).toBe(false)
    expect(s.adopt(undefined, [[1, 2, 3, 4]], [WALL])).toBe(true)
    expect(s.adopt(undefined, [[1, 2, 3, 5]])).toBe(true)
  })

  it('fetches only the window around the player and drops far tiles', async () => {
    const { asked, fetcher } = server([[100, 100, 120, 120], [5000, 5000, 5020, 5020], [20000, 20000, 20030, 20030]])
    const s = new WallStore(30000, meta, [], [], fetcher)
    expect(s.ready).toBe(false)
    s.update(110, 110)
    await flush()
    expect(asked.length).toBe(1)
    expect(new URL(asked[0]!, 'http://x').searchParams.get('c')!.split(';').length).toBe(9) // 3x3 of the 5x5 at the corner
    expect(s.list()).toEqual([[100, 100, 120, 120]])
    expect(s.ready).toBe(true)
    // Same tile again asks for nothing.
    s.update(111, 111)
    expect(asked.length).toBe(1)
    // Far away: new window fetched, the old walls let go.
    s.update(20010, 20010)
    await flush()
    expect(s.list()).toEqual([[20000, 20000, 20030, 20030]])
  })

  it('counts a wall on a tile border once', async () => {
    const { fetcher } = server([[250, 250, 262, 262]])
    const s = new WallStore(2000, meta, [], [], fetcher)
    s.update(250, 250)
    await flush()
    expect(s.list().length).toBe(1)
  })

  it('ignores a reply for other walls and starts over when the server says they changed', async () => {
    const stale = async () => encode(0xbbn, [['0,0', [[0, 1, 1, 2, 2]]]])
    const t = new WallStore(2000, meta, [], [], stale)
    t.update(10, 10)
    await flush()
    expect(t.list()).toEqual([])

    const { fetcher } = server([[100, 100, 120, 120]])
    const s = new WallStore(2000, meta, [], [], fetcher)
    s.update(110, 110)
    await flush()
    const rev = s.rev
    expect(s.adopt(meta, [])).toBe(false)
    expect(s.adopt({ ...meta, ver: '00000000000000bb' }, [])).toBe(true)
    expect(s.list()).toEqual([])
    expect(s.rev).toBeGreaterThan(rev)
  })
})
