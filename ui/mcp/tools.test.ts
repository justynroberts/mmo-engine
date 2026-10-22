import { materialChar, overlay, shapeCells, TOOLS } from './tools'
import type { Ctx } from './tools'

describe('building with blocks', () => {
  it('turns world rectangles into block cells, solid or as an outline', () => {
    expect(shapeCells('fill', 0, 0, 5, 3)).toHaveLength(3 * 2)
    const walls = shapeCells('walls', 0, 0, 9, 9)
    expect(walls).toHaveLength(16)
    expect(walls).not.toContainEqual([2, 2])
  })
  it('draws a line cell by cell without repeats', () => {
    const l = shapeCells('line', 0, 0, 10, 4)
    expect(l[0]).toEqual([0, 0])
    expect(l[l.length - 1]).toEqual([5, 2])
    expect(new Set(l.map((c) => c.join())).size).toBe(l.length)
  })
  it('refuses a shape too big for one call', () => {
    expect(() => shapeCells('fill', 0, 0, 1000, 1000)).toThrow(/at most/)
  })
  it('lays layers over a column and keeps the rest', () => {
    expect(overlay('', 0, 3, '0')).toBe('000')
    expect(overlay('000', 3, 1, '6')).toBe('0006')
    expect(overlay('0006', 0, 2, '.')).toBe('..06')
    expect(overlay('0', 0, 1, '.')).toBe('')
  })
  it('knows materials by name or number', () => {
    expect(materialChar('stone')).toBe('0')
    expect(materialChar('Fire')).toBe('d')
    expect(materialChar(11)).toBe('b')
    expect(() => materialChar('cheese')).toThrow(/Materials:/)
  })
})

describe('editing the game file', () => {
  const game = () => ({
    move_speed: 20, entity_radius: 0.5, interest_radius: 128, max_visible: 64,
    attrs: [{ name: 'health', min: 0, max: 100, default: 100, bits: 8, replicated: true, regen_per_sec: 0 }],
    actions: [], archetypes: [{ name: 'wolf', color: '#888', count: 2, behaviour: 'wander', speed_mult: 1, sight: 40, attack: null, attrs: {}, home: 'den', rules: [] }],
    places: [{ name: 'den', x: 10, y: 10, r: 5 }], routes: [],
  })
  function fake() {
    let g: any = game()
    const puts: any[] = []
    const ctx: Ctx = {
      async call(path, method = 'GET', body) {
        if (path === '/api/game' && method === 'GET') return { status: 200, body: g }
        if (path === '/api/game' && method === 'PUT') return puts.push(body), (g = body), { status: 200, body: { ok: true } }
        return { status: 404, body: { ok: false, error: 'not here' } }
      },
      setBase() {},
      base: () => 'test',
    }
    return { ctx, puts, game: () => g }
  }
  const tool = (n: string) => TOOLS.find((t) => t.name === n)!

  it('removing a place drops the homes that pointed at it, as the console does', async () => {
    const f = fake()
    await tool('remove').run({ section: 'places', name: 'den' }, f.ctx)
    expect(f.game().places).toEqual([])
    expect(f.game().archetypes[0].home).toBeNull()
  })
  it('will not restart the world unless told to', async () => {
    const f = fake()
    await expect(tool('upsert').run({ section: 'attrs', entry: { name: 'mana' } }, f.ctx)).rejects.toThrow(/allow_restart/)
    expect(f.puts).toHaveLength(0)
    await tool('upsert').run({ section: 'attrs', entry: { name: 'mana' }, allow_restart: true }, f.ctx)
    expect(f.game().attrs.map((a: any) => a.name)).toEqual(['health', 'mana'])
  })
  it('names what is missing when an entry is not there', async () => {
    const f = fake()
    await expect(tool('rename').run({ section: 'archetypes', from: 'bear', to: 'big bear' }, f.ctx)).rejects.toThrow(/It has: wolf/)
  })
})
