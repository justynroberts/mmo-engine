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
  it('applies spell fields live, without a restart, and leaves defaults out', async () => {
    const f = fake()
    await tool('upsert').run(
      { section: 'actions', entry: { name: 'bolt', range: 20, effects: [{ to: 'target', attr: 'health', delta: -5 }] }, allow_restart: true },
      f.ctx,
    )
    const spell = { name: 'bolt', aim: 'target', area: 6, hits: 'foes', hold: { kind: 'root', secs: 2 }, fx: 'frost', bolt: true, school: 'black',
      effects: [{ to: 'target', attr: 'health', delta: -5, secs: 4 }] }
    await tool('upsert').run({ section: 'actions', entry: spell }, f.ctx)
    const a = f.game().actions[0]
    expect(a).toMatchObject({ area: 6, hits: 'foes', hold: { kind: 'root', secs: 2 }, fx: 'frost', bolt: true, school: 'black' })
    expect(a.effects[0].secs).toBe(4)
    expect('aim' in a).toBe(false)
    await tool('upsert').run({ section: 'actions', entry: { name: 'bolt', hold: null, fx: '', bolt: false, area: 0, hits: 'any',
      effects: [{ to: 'target', attr: 'health', delta: -5, secs: 0 }] } }, f.ctx)
    const b = f.game().actions[0]
    for (const k of ['hold', 'fx', 'bolt', 'area', 'hits']) expect(k in b).toBe(false)
    expect('secs' in b.effects[0]).toBe(false)
    await expect(tool('upsert').run({ section: 'actions', entry: { name: 'bolt', range: 30 } }, f.ctx)).rejects.toThrow(/allow_restart/)
  })
  it('describes spells in the schema', async () => {
    const r: any = await tool('game_schema').run({}, fake().ctx)
    expect(r.spells).toMatch(/held/)
    expect(r.fx).toContain('frost')
    expect(r.types).toMatch(/hold\?: HoldFile/)
  })
  it('writes quests live, leaving optional fields out, and keeps them right on rename and remove', async () => {
    const f = fake()
    f.game().items = [{ name: 'pelt', color: '#a50', stack_max: 5, uses: [], consume: false }]
    const quest = {
      name: 'cull', title: 'The cull', text: 'Thin the wolves.', giver: 'wolf', after: null, xp: 120,
      steps: [{ kind: 'kill', archetype: 'wolf', count: 6 }, { kind: 'visit', place: 'den' }, { kind: 'collect', item: 'pelt', count: 3 }],
      reward: [{ item: 'pelt', count: 1 }], repeatable: false,
    }
    // No allow_restart: quests are not in the content hash.
    const r: any = await tool('upsert').run({ section: 'quests', entry: quest }, f.ctx)
    expect(r.restarted).toBe(false)
    const q = f.game().quests[0]
    expect(q).toMatchObject({ name: 'cull', giver: 'wolf', xp: 120, reward: [{ item: 'pelt', count: 1 }] })
    for (const k of ['after', 'repeatable', 'turn_in', 'done_text', 'level']) expect(k in q).toBe(false)
    await tool('upsert').run({ section: 'quests', entry: { name: 'cull', title: 'The great cull' } }, f.ctx)
    expect(f.game().quests[0]).toMatchObject({ title: 'The great cull', xp: 120 })
    await tool('rename').run({ section: 'archetypes', from: 'wolf', to: 'mac tire' }, f.ctx)
    expect(f.game().quests[0].giver).toBe('mac tire')
    expect(f.game().quests[0].steps[0].archetype).toBe('mac tire')
    await tool('rename').run({ section: 'places', from: 'den', to: 'lair' }, f.ctx)
    expect(f.game().quests[0].steps[1].place).toBe('lair')
    await tool('remove').run({ section: 'items', name: 'pelt' }, f.ctx)
    expect(f.game().quests[0].steps.map((s: any) => s.kind)).toEqual(['kill', 'visit'])
    expect('reward' in f.game().quests[0]).toBe(false)
    await tool('remove').run({ section: 'archetypes', name: 'mac tire' }, f.ctx)
    expect(f.game().quests).toEqual([])
  })
  it('refuses a quest that names what is not there', async () => {
    const f = fake()
    await expect(
      tool('upsert').run({ section: 'quests', entry: { name: 'q', title: 'Q', text: '', giver: 'bear', steps: [{ kind: 'visit', place: 'den' }] } }, f.ctx),
    ).rejects.toThrow(/no bot type called “bear”/)
    expect(f.puts).toHaveLength(0)
  })
  it('describes quests and experience in the schema', async () => {
    const r: any = await tool('game_schema').run({}, fake().ctx)
    expect(r.quests).toMatch(/apply live/)
    expect(r.quests).toMatch(/discover_xp/)
    expect(r.types).toMatch(/export interface QuestFile/)
  })
  it('writes classes live, leaving optional fields out, and keeps their spells right on rename and remove', async () => {
    const f = fake()
    const zap = (name: string) => ({ name, range: 8, effects: [{ to: 'target', attr: 'health', delta: -5 }] })
    for (const a of ['strike', 'liathroid tine', 'sioc']) await tool('upsert').run({ section: 'actions', entry: zap(a), allow_restart: true }, f.ctx)
    // No allow_restart: classes are not in the content hash.
    const cls = { name: 'draoi', title: 'Draoi (Druid)', desc: 'Calls on the old powers.', look: 'mage', might: 0.15, toughness: 0, spells: ['liathroid tine', 'sioc'] }
    const r: any = await tool('upsert').run({ section: 'classes', entry: cls }, f.ctx)
    expect(r.restarted).toBe(false)
    expect(f.game().classes[0]).toEqual({ name: 'draoi', title: 'Draoi (Druid)', desc: 'Calls on the old powers.', look: 'mage', might: 0.15, spells: ['liathroid tine', 'sioc'] })
    await tool('upsert').run({ section: 'classes', entry: { name: 'draoi', desc: null } }, f.ctx)
    expect('desc' in f.game().classes[0]).toBe(false)
    await expect(tool('upsert').run({ section: 'classes', entry: { name: 'bad', title: 'Bad', spells: ['nope'] } }, f.ctx)).rejects.toThrow(/“nope” is not an action/)
    await tool('rename').run({ section: 'actions', from: 'sioc', to: 'frost', allow_restart: true }, f.ctx)
    expect(f.game().classes[0].spells).toEqual(['liathroid tine', 'frost'])
    await tool('remove').run({ section: 'actions', name: 'liathroid tine', allow_restart: true }, f.ctx)
    expect(f.game().classes[0].spells).toEqual(['frost'])
    await tool('rename').run({ section: 'classes', from: 'draoi', to: 'druid' }, f.ctx)
    expect(f.game().classes.map((c: any) => c.name)).toEqual(['druid'])
    await tool('remove').run({ section: 'classes', name: 'druid' }, f.ctx)
    expect(f.game().classes).toEqual([])
  })
  it('describes classes in the schema', async () => {
    const r: any = await tool('game_schema').run({}, fake().ctx)
    expect(r.classes).toMatch(/section "classes"/)
    expect(r.classes).toMatch(/reserved attribute "class"/)
    expect(r.classes).toMatch(/picks a class once, in game/)
    expect(r.classes).toMatch(/apply live/)
    expect(r.types).toMatch(/export interface ClassFile/)
  })
  it('sets the currency, sells in shops, and keeps both right when the item is renamed or removed', async () => {
    const f = fake()
    await tool('upsert').run({ section: 'items', entry: { name: 'coin', stack_max: 99 } }, f.ctx)
    await tool('upsert').run({ section: 'items', entry: { name: 'sword', stack_max: 1, value: 20, equip: { slot: 'weapon', might: 0.2 } } }, f.ctx)
    await expect(tool('set_settings').run({ currency: 'gold' }, f.ctx)).rejects.toThrow(/no item called “gold”/)
    const r: any = await tool('set_settings').run({ currency: 'coin' }, f.ctx)
    expect(r.settings.currency).toBe('coin')
    await tool('upsert').run({ section: 'archetypes', entry: { name: 'wolf', shop: [{ item: 'sword', price: 50 }] } }, f.ctx)
    expect(f.game().archetypes[0].shop).toEqual([{ item: 'sword', price: 50 }])
    await tool('rename').run({ section: 'items', from: 'coin', to: 'crown' }, f.ctx)
    expect(f.game().currency).toBe('crown')
    await tool('remove').run({ section: 'items', name: 'sword' }, f.ctx)
    expect('shop' in f.game().archetypes[0]).toBe(false)
    await tool('set_settings').run({ currency: null }, f.ctx)
    expect('currency' in f.game()).toBe(false)
    const schema: any = await tool('game_schema').run({}, f.ctx)
    expect(schema.trade).toMatch(/\/invite Name/)
    expect(schema.types).toMatch(/export interface ShopItem/)
  })
  it('names what is missing when an entry is not there', async () => {
    const f = fake()
    await expect(tool('rename').run({ section: 'archetypes', from: 'bear', to: 'big bear' }, f.ctx)).rejects.toThrow(/It has: wolf/)
  })
})
