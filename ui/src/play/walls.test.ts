import { describe, expect, it } from 'vitest'
import { WallStore } from './walls'

const meta = { ver: 'aa', chunk: 256, count: 3, inline: false }
const flush = () => new Promise((r) => setTimeout(r, 0))

function server(boxes: [number, number, number, number][]) {
  const asked: string[] = []
  const fetcher = async (url: string) => {
    asked.push(url)
    const c = new URL(url, 'http://x').searchParams.get('c')!.split(';')
    const chunks: Record<string, number[][]> = {}
    for (const k of c) {
      const [cx, cy] = k.split(',').map(Number) as [number, number]
      chunks[k] = boxes
        .map((b, i) => [i, ...b])
        .filter((b) => b[3]! >= cx * 256 && b[1]! < (cx + 1) * 256 && b[4]! >= cy * 256 && b[2]! < (cy + 1) * 256)
    }
    return { ver: 'aa', chunk: 256, chunks }
  }
  return { asked, fetcher }
}

describe('WallStore', () => {
  it('holds inline walls and notices when they change', () => {
    const s = new WallStore(1000, { ver: 'x', chunk: 256, count: 1, inline: true }, [[1, 2, 3, 4]])
    expect(s.list()).toEqual([[1, 2, 3, 4]])
    expect(s.isStreaming).toBe(false)
    expect(s.adopt(undefined, [[1, 2, 3, 4]])).toBe(false)
    expect(s.adopt(undefined, [[1, 2, 3, 5]])).toBe(true)
  })

  it('fetches only the window around the player and drops far chunks', async () => {
    const { asked, fetcher } = server([[100, 100, 120, 120], [5000, 5000, 5020, 5020], [20000, 20000, 20030, 20030]])
    const s = new WallStore(30000, meta, [], fetcher)
    expect(s.ready).toBe(false)
    s.update(110, 110)
    await flush()
    expect(asked.length).toBe(1)
    expect(asked[0]!.split(';').length).toBe(9) // clamped at the world corner: 3x3 of the 5x5 window
    expect(s.list()).toEqual([[100, 100, 120, 120]])
    expect(s.ready).toBe(true)
    // Same chunk again asks for nothing.
    s.update(111, 111)
    expect(asked.length).toBe(1)
    // Far away: new window fetched, the old walls let go.
    s.update(20010, 20010)
    await flush()
    expect(s.list()).toEqual([[20000, 20000, 20030, 20030]])
  })

  it('counts a wall on a chunk border once', async () => {
    const { fetcher } = server([[250, 250, 262, 262]])
    const s = new WallStore(2000, meta, [], fetcher)
    s.update(250, 250)
    await flush()
    expect(s.list().length).toBe(1)
  })

  it('starts over when the server says the walls changed', async () => {
    const { fetcher } = server([[100, 100, 120, 120]])
    const s = new WallStore(2000, meta, [], fetcher)
    s.update(110, 110)
    await flush()
    const rev = s.rev
    expect(s.adopt(meta, [])).toBe(false)
    expect(s.adopt({ ...meta, ver: 'bb' }, [])).toBe(true)
    expect(s.list()).toEqual([])
    expect(s.rev).toBeGreaterThan(rev)
  })
})
