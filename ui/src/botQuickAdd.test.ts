import { fixture } from './test/fixture'
import { addFromTemplate, BOT_TEMPLATES, exportArchetype, importArchetypes } from './gameEdit'

describe('adding bots quickly', () => {
  it('every template only refers to things that exist in the game', () => {
    const g = fixture()
    for (const t of BOT_TEMPLATES) {
      const a = t.make(g)
      if (a.attack) expect(g.actions.map((x) => x.name)).toContain(a.attack)
      for (const r of a.rules) if (r.then.kind === 'patrol') expect(g.routes.map((x) => x.name)).toContain(r.then.route)
      expect(g.archetypes.map((x) => x.name)).not.toContain(a.name)
    }
  })

  it('adding the same template twice gives distinct names', () => {
    let g = fixture()
    g = addFromTemplate(g, 'guard')
    g = addFromTemplate(g, 'guard')
    const names = g.archetypes.map((a) => a.name)
    expect(new Set(names).size).toBe(names.length)
  })

  it('a template works even in a game with no actions or routes', () => {
    const g = { ...fixture(), actions: [], routes: [] }
    for (const t of BOT_TEMPLATES) expect(() => t.make(g)).not.toThrow()
    expect(addFromTemplate(g, 'guard').archetypes.at(-1)!.attack).toBeNull()
  })

  it('export then import round-trips and replaces by name', () => {
    const g = fixture()
    const a = { ...g.archetypes[0]!, count: 9 }
    const r = importArchetypes(g, exportArchetype(a))
    expect('game' in r && r.game.archetypes.find((x) => x.name === a.name)!.count).toBe(9)
    expect('game' in r && r.game.archetypes.length).toBe(g.archetypes.length)
  })

  it('accepts a minimal object and an array, filling in the defaults', () => {
    const g = fixture()
    const r = importArchetypes(g, '[{"name":"a1"},{"name":"a2","count":2,"behaviour":"chase"}]')
    expect('game' in r && r.names).toEqual(['a1', 'a2'])
    expect('game' in r && r.game.archetypes.at(-1)!.count).toBe(2)
    expect('game' in r && r.game.archetypes.at(-2)!.speed_mult).toBe(1)
  })

  it.each([
    ['{not json', 'not valid JSON'],
    ['[]', 'nothing'],
    ['{"colour":"#fff","name":"x"}', 'colour'],
    ['{"name":"x","behaviour":"dance"}', 'behaviour'],
    ['{"name":"x","attack":"nope"}', 'no action called'],
    ['{"name":"x","home":"nowhere"}', 'no place called'],
    ['{"name":"x","attrs":{"mana":3}}', 'no attribute called'],
    ['{"count":3}', 'needs a name'],
  ])('refuses %s with a useful message', (text, part) => {
    const g = fixture()
    const r = importArchetypes(g, text)
    expect('error' in r && r.error).toContain(part)
  })
})
