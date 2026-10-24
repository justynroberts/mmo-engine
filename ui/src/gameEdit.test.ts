import type { GameFile } from './api'
import { directorProblems, removeArch, removeItem, renameArch, renameItem } from './directorEdit'
import { fixture } from './test/fixture'
import {
  addWall,
  cleanAction,
  removeFaction,
  renameFaction,
  generatedWalls,
  MAX_WALLS,
  needsReset,
  normWall,
  removeWall,
  setWallEdge,
  takeOverWalls,
  newAction,
  newArchetype,
  newAttr,
  parseNum,
  removeAction,
  removeAttr,
  renameAction,
  renameAttr,
  uniqueName,
  cleanQuest,
  classProblems,
  cleanClass,
  newClass,
  removeClass,
  renameClass,
  newQuest,
  questProblems,
  removePlace,
  removeQuest,
  renamePlace,
  renameQuest,
} from './gameEdit'

describe('renaming', () => {
  it('renames an attribute everywhere it is referenced', () => {
    const g = renameAttr(fixture(), 0, 'hp')
    expect(g.attrs[0]?.name).toBe('hp')
    expect(g.actions[0]?.effects[0]?.attr).toBe('hp')
    expect(g.archetypes[0]?.attrs).toEqual({ hp: 60 })
    // Untouched references stay put.
    expect(g.actions[0]?.requires?.attr).toBe('energy')
  })
  it('renames the requirement too', () => {
    const g = renameAttr(fixture(), 1, 'mana')
    expect(g.actions[0]?.requires?.attr).toBe('mana')
    expect(g.actions[0]?.effects[1]?.attr).toBe('mana')
  })
  it('renaming to the same name or an unknown index is a no-op', () => {
    const f = fixture()
    expect(renameAttr(f, 0, 'health')).toBe(f)
    expect(renameAttr(f, 9, 'x')).toBe(f)
  })
  it('renames an action and the bots that attack with it', () => {
    const g = renameAction(fixture(), 0, 'shock')
    expect(g.actions[0]?.name).toBe('shock')
    expect(g.archetypes[0]?.attack).toBe('shock')
  })
})

describe('removing', () => {
  it('removing an attribute strips every reference so the file stays valid', () => {
    const g = removeAttr(fixture(), 1)
    expect(g.attrs.map((a) => a.name)).toEqual(['health'])
    expect(g.actions[0]?.requires).toBeNull()
    expect(g.actions[0]?.effects.map((e) => e.attr)).toEqual(['health'])
    const h = removeAttr(fixture(), 0)
    expect(h.archetypes[0]?.attrs).toEqual({})
  })
  it('removing an action clears bots that used it', () => {
    const g = removeAction(fixture(), 0)
    expect(g.actions).toHaveLength(0)
    expect(g.archetypes[0]?.attack).toBeNull()
  })
})

describe('defaults for new things', () => {
  it('never reuse a name or an action id', () => {
    const f = fixture()
    expect(newAttr(f).name).toBe('attr')
    f.attrs.push({ ...newAttr(f) })
    expect(newAttr(f).name).toBe('attr_2')
    expect(newAction(f).id).toBe(2)
    expect(newArchetype(f).name).toBe('bot')
    expect(uniqueName('a', ['a', 'a_2', 'a_3'])).toBe('a_4')
  })
})

describe('needsReset', () => {
  it('is false for bot-only edits and true for anything that changes the wire format', () => {
    const live = fixture()
    const bots = fixture()
    bots.archetypes[0]!.count = 99
    bots.archetypes[0]!.color = '#000000'
    expect(needsReset(live, bots)).toBe(false)
    for (const edit of [
      (g: ReturnType<typeof fixture>) => (g.move_speed = 30),
      (g: ReturnType<typeof fixture>) => (g.attrs[0]!.max = 200),
      (g: ReturnType<typeof fixture>) => (g.actions[0]!.range = 9),
      (g: ReturnType<typeof fixture>) => g.attrs.pop(),
    ]) {
      const g = fixture()
      edit(g)
      expect(needsReset(live, g)).toBe(true)
    }
  })
  it('is false for spell fields, which apply live', () => {
    const live = fixture()
    const g = fixture()
    Object.assign(g.actions[0]!, { aim: 'self', area: 8, hits: 'friends', hold: { kind: 'stun', secs: 2 }, fx: 'holy', bolt: true, school: 'white' })
    g.actions[0]!.effects[0]!.secs = 5
    expect(needsReset(live, g)).toBe(false)
    g.actions[0]!.effects[0]!.delta += 1
    expect(needsReset(live, g)).toBe(true)
  })
})

describe('cleanAction', () => {
  it('leaves spell defaults out so the game file round-trips clean', () => {
    const a = fixture().actions[0]!
    const set = cleanAction({ ...a, area: 4, fx: 'fire', effects: a.effects.map((e) => ({ ...e, secs: 3 })) })
    expect(set).toMatchObject({ area: 4, fx: 'fire' })
    const back = cleanAction({ ...set, aim: 'target', area: 0, hits: 'any', hold: undefined, fx: undefined, bolt: false, school: '',
      effects: set.effects.map((e) => ({ ...e, secs: 0 })) })
    expect(JSON.stringify(back)).toBe(JSON.stringify(a))
  })
})

describe('parseNum', () => {
  it('accepts numbers and rejects half-typed input', () => {
    expect(parseNum('12.5')).toBe(12.5)
    expect(parseNum('-3')).toBe(-3)
    expect(parseNum('1e2')).toBe(100)
    for (const bad of ['', ' ', '-', '.', 'abc', '1,5', 'NaN', 'Infinity']) expect(parseNum(bad)).toBeNull()
  })
})

describe('walls', () => {
  it('turns a dragged rectangle into a sorted, rounded, in-world wall and ignores accidental clicks', () => {
    expect(normWall(50.4, 90, 10, 20.6, 512)).toEqual([10, 21, 50, 90])
    expect(normWall(-20, -5, 600, 30, 512)).toEqual([0, 0, 512, 30])
    expect(normWall(10, 10, 11, 80, 512)).toBeNull()
    expect(normWall(10, 10, 80, 12, 512)).toBeNull()
  })

  it('takes over the generated walls as a copy, edits them, and can hand them back', () => {
    const base = { ...fixture() }
    const cur: [number, number, number, number][] = [[1, 2, 30, 40]]
    let g = takeOverWalls(base, cur)
    cur[0]![0] = 99
    expect(g.walls).toEqual([[1, 2, 30, 40]])
    g = addWall(g, [100, 100, 120, 160])
    expect(g.walls).toHaveLength(2)
    g = setWallEdge(g, 0, 2, 25, 512)
    expect(g.walls![0]).toEqual([1, 2, 25, 40])
    // An edge cannot cross the opposite one or leave the world.
    expect(setWallEdge(g, 0, 0, 500, 512).walls![0]![0]).toBe(24)
    expect(setWallEdge(g, 0, 3, 9999, 512).walls![0]![3]).toBe(512)
    g = removeWall(g, 0)
    expect(g.walls).toEqual([[100, 100, 120, 160]])
    expect('walls' in generatedWalls(g)).toBe(false)
  })

  it('does not count a wall change as a schema change (nobody is disconnected)', () => {
    const live = fixture()
    expect(needsReset(live, takeOverWalls(live, [[1, 2, 30, 40]]))).toBe(false)
  })

  it('stops adding at the limit', () => {
    const g = { ...fixture(), walls: Array.from({ length: MAX_WALLS }, () => [0, 0, 5, 5] as [number, number, number, number]) }
    expect(addWall(g, [1, 1, 9, 9]).walls).toHaveLength(MAX_WALLS)
  })
})

describe('quests', () => {
  const withQuests = (): GameFile => ({
    ...fixture(),
    archetypes: [
      ...fixture().archetypes,
      { name: 'elder', color: '#fff', count: 1, behaviour: 'idle', speed_mult: 1, sight: 20, attack: null, attrs: {}, home: null, rules: [] },
    ],
    places: [{ name: 'den', x: 10, y: 10, r: 5 }],
    items: [{ name: 'pelt', color: '#a50', stack_max: 5, uses: [], consume: false }],
    quests: [
      {
        name: 'cull',
        title: 'The cull',
        text: 'Thin them.',
        giver: 'elder',
        turn_in: 'drone',
        steps: [
          { kind: 'kill', archetype: 'drone', count: 6 },
          { kind: 'visit', place: 'den' },
          { kind: 'collect', item: 'pelt', count: 3 },
        ],
        xp: 120,
        reward: [{ item: 'pelt', count: 1 }],
      },
      { name: 'after', title: 'Then', text: '', giver: 'drone', after: 'cull', steps: [{ kind: 'talk', archetype: 'elder' }] },
    ],
  })

  it('are not in the content hash: editing them never restarts the world', () => {
    const g = withQuests()
    expect(needsReset(g, { ...g, quests: [] })).toBe(false)
    expect(needsReset(g, { ...g, levels: { cap: 10 } })).toBe(false)
  })
  it('a new quest names a bot type that exists, and none without one', () => {
    const q = newQuest(withQuests())!
    expect(q.giver).toBe('drone')
    expect(q.steps).toEqual([{ kind: 'visit', place: 'den' }])
    expect(questProblems({ ...withQuests(), quests: [q] })).toEqual([])
    expect(newQuest({ ...fixture(), archetypes: [] })).toBeNull()
  })
  it('leaves optional fields out when empty or default', () => {
    const q = cleanQuest({ name: 'q', title: 't', text: '', giver: 'a', turn_in: 'a', level: 1, after: '', done_text: '', xp: 0, reward: [], repeatable: false, steps: [] })
    expect(Object.keys(q).sort()).toEqual(['giver', 'name', 'steps', 'text', 'title'])
  })
  it('renaming a bot type, place or item rewrites the quests', () => {
    let g = renameArch(withQuests(), 0, 'wolf')
    expect(g.quests?.[0]?.turn_in).toBe('wolf')
    expect(g.quests?.[0]?.steps[0]).toEqual({ kind: 'kill', archetype: 'wolf', count: 6 })
    expect(g.quests?.[1]?.giver).toBe('wolf')
    g = renameArch(g, 1, 'sage')
    expect(g.quests?.[0]?.giver).toBe('sage')
    expect(g.quests?.[1]?.steps[0]).toEqual({ kind: 'talk', archetype: 'sage' })
    g = renamePlace(g, 0, 'lair')
    expect(g.quests?.[0]?.steps[1]).toEqual({ kind: 'visit', place: 'lair' })
    g = renameItem(g, 0, 'hide')
    expect(g.quests?.[0]?.steps[2]).toEqual({ kind: 'collect', item: 'hide', count: 3 })
    expect(g.quests?.[0]?.reward).toEqual([{ item: 'hide', count: 1 }])
    expect(questProblems(g)).toEqual([])
  })
  it('removing things drops the steps and rewards that needed them', () => {
    let g = removePlace(withQuests(), 0)
    expect(g.quests?.[0]?.steps.map((s) => s.kind)).toEqual(['kill', 'collect'])
    g = removeItem(g, 0)
    expect(g.quests?.[0]?.steps.map((s) => s.kind)).toEqual(['kill'])
    expect(g.quests?.[0] && 'reward' in g.quests[0]).toBe(false)
    // The turn-in bot type goes: it falls back to the giver. Its kill step goes, leaving no steps: the quest goes,
    // and the one after it stops waiting for it.
    g = removeArch(g, 0)
    expect(g.quests?.map((q) => q.name)).toEqual([])
  })
  it('removing a giver drops its quest, and the next one stops waiting for it', () => {
    const g = removeArch(withQuests(), 1)
    // "cull" goes with its giver; "after" loses its only step (talk to elder) and goes too.
    expect(g.quests).toEqual([])
    const h = removeArch({ ...withQuests(), quests: [withQuests().quests![0]!, { ...withQuests().quests![1]!, steps: [{ kind: 'visit', place: 'den' }] }] }, 1)
    expect(h.quests?.map((q) => q.name)).toEqual(['after'])
    expect(h.quests?.[0] && 'after' in h.quests[0]).toBe(false)
  })
  it('a removed turn-in bot type falls back to the giver', () => {
    const g = withQuests()
    g.quests![0]!.steps = [{ kind: 'visit', place: 'den' }]
    const h = removeArch(g, 0)
    expect(h.quests?.[0]?.giver).toBe('elder')
    expect(h.quests?.[0] && 'turn_in' in h.quests[0]).toBe(false)
  })
  it('renaming and removing a quest keeps "after" right', () => {
    let g = renameQuest(withQuests(), 0, 'the cull')
    expect(g.quests?.[1]?.after).toBe('the cull')
    g = removeQuest(g, 0)
    expect(g.quests?.map((q) => q.name)).toEqual(['after'])
    expect(g.quests?.[0] && 'after' in g.quests[0]).toBe(false)
  })
  it('reports what the server would refuse', () => {
    const g = withQuests()
    g.quests![1]!.after = 'nope'
    g.quests![1]!.steps = []
    expect(questProblems(g).join('\n')).toMatch(/not another quest[\s\S]*1 to 8 steps/)
    expect(directorProblems(g).some((p) => p.includes('quest after'))).toBe(true)
  })
})

describe('trade', () => {
  const traded = (): GameFile => ({
    ...fixture(),
    items: [
      { name: 'coin', color: '#fd0', stack_max: 99, uses: [], consume: false },
      { name: 'sword', color: '#ccc', stack_max: 1, uses: [], consume: false, value: 20, equip: { slot: 'weapon', might: 0.2 } },
      { name: 'horse', color: '#a60', stack_max: 1, uses: [], consume: false, mount: 0.8 },
    ],
    currency: 'coin',
    archetypes: fixture().archetypes.map((a, i) => (i === 0 ? { ...a, shop: [{ item: 'sword', price: 50 }, { item: 'horse', price: 200 }] } : a)),
  })
  it('renaming an item renames it in shops and the currency', () => {
    let g = renameItem(traded(), 1, 'blade')
    expect(g.archetypes[0]?.shop).toEqual([{ item: 'blade', price: 50 }, { item: 'horse', price: 200 }])
    g = renameItem(g, 0, 'crown')
    expect(g.currency).toBe('crown')
    expect(directorProblems(g)).toEqual([])
  })
  it('removing an item drops it from shops, and clears the currency if it was the money', () => {
    let g = removeItem(traded(), 1)
    expect(g.archetypes[0]?.shop).toEqual([{ item: 'horse', price: 200 }])
    expect(g.currency).toBe('coin')
    g = removeItem(g, 1)
    expect('shop' in (g.archetypes[0] ?? {})).toBe(false)
    g = removeItem(g, 0)
    expect('currency' in g).toBe(false)
    expect(directorProblems(g)).toEqual([])
  })
  it('names shop items and a currency that are not items', () => {
    const g = { ...traded(), items: [] }
    expect(directorProblems(g)).toEqual(expect.arrayContaining([expect.stringMatching(/sells “sword”/), expect.stringMatching(/currency “coin”/)]))
  })
  it('none of it restarts the world', () => {
    const live = fixture()
    expect(needsReset(live, traded())).toBe(false)
  })
})

describe('classes', () => {
  const withClasses = (): GameFile => ({
    ...fixture(),
    actions: [
      ...fixture().actions,
      { id: 2, name: 'fire', cooldown_ticks: 10, range: 8, requires: null, effects: [{ to: 'target', attr: 'health', delta: -20 }] },
      { id: 3, name: 'frost', cooldown_ticks: 10, range: 8, requires: null, effects: [{ to: 'target', attr: 'health', delta: -15 }] },
    ],
    classes: [
      { name: 'draoi', title: 'Draoi (Druid)', look: 'mage', might: 0.15, spells: ['fire', 'frost'] },
      { name: 'laoch', title: 'Laoch', look: 'knight', toughness: 0.3, spells: ['frost'] },
      { name: 'any', title: 'Anyone' },
    ],
  })

  it('are not in the content hash: editing them never restarts the world', () => {
    const g = withClasses()
    expect(needsReset(g, { ...g, classes: [] })).toBe(false)
  })
  it('renaming an action renames it in every class that may use it', () => {
    const g = renameAction(withClasses(), 2, 'sioc')
    expect(g.classes?.map((c) => c.spells)).toEqual([['fire', 'sioc'], ['sioc'], undefined])
    expect(classProblems(g)).toEqual([])
  })
  it('removing an action drops it from the classes, leaving optional fields out', () => {
    const g = removeAction(withClasses(), 2)
    expect(g.classes?.[0]?.spells).toEqual(['fire'])
    expect(g.classes?.[1] && 'spells' in g.classes[1]).toBe(false)
    expect(g.classes?.[2]).toEqual({ name: 'any', title: 'Anyone' })
    expect(classProblems(g)).toEqual([])
  })
  it('a game without classes stays without them', () => {
    expect('classes' in renameAction(fixture(), 0, 'shock')).toBe(false)
    expect('classes' in removeAction(fixture(), 0)).toBe(false)
  })
  it('adds, renames and removes classes', () => {
    const g = withClasses()
    expect(newClass(g)).toEqual({ name: 'class', title: 'A new class' })
    expect(renameClass(g, 0, 'druid').classes?.[0]?.name).toBe('druid')
    expect(renameClass(g, 0, 'draoi')).toBe(g)
    expect(removeClass(g, 1).classes?.map((c) => c.name)).toEqual(['draoi', 'any'])
  })
  it('leaves empty and zero fields out', () => {
    expect(cleanClass({ name: 'x', title: 'X', desc: '', look: '', might: 0, toughness: 0, spells: [] })).toEqual({ name: 'x', title: 'X' })
  })
  it('says what the server would refuse', () => {
    const g = withClasses()
    expect(classProblems(g)).toEqual([])
    const bad: GameFile = {
      ...g,
      classes: [
        { name: 'a', title: '', might: 3 },
        { name: 'a', title: 'A', spells: ['nope'] },
      ],
    }
    const p = classProblems(bad).join('\n')
    expect(p).toMatch(/a title of 1 to 32/)
    expect(p).toMatch(/might and toughness from 0 to 2/)
    expect(p).toMatch(/two classes are called “a”/)
    expect(p).toMatch(/“nope” is not an action/)
    expect(directorProblems(bad).length).toBeGreaterThan(0)
    expect(classProblems({ ...g, classes: Array.from({ length: 16 }, (_, i) => ({ name: `c${i}`, title: 'C' })) })).toContain('at most 15 classes')
  })
})

describe('factions', () => {
  it('renaming or removing one carries its bot types and haters with it', () => {
    const g = {
      factions: [{ name: 'folk', hates: ['raiders'] }, { name: 'raiders' }],
      archetypes: [{ name: 'farmer', faction: 'folk' }, { name: 'brigand', faction: 'raiders' }],
    } as unknown as GameFile
    const r = renameFaction(g, 1, 'brigands')
    expect(r.factions?.[0]?.hates).toEqual(['brigands'])
    expect(r.archetypes[1]?.faction).toBe('brigands')
    const d = removeFaction(r, 1)
    expect(d.factions!.map((f) => f.name)).toEqual(['folk'])
    expect(d.factions?.[0]?.hates).toEqual([])
    expect("faction" in (d.archetypes[1] ?? {})).toBe(false)
    expect(d.archetypes[0]?.faction).toBe('folk')
  })
})
