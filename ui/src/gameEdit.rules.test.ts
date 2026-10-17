import type { GameFile } from './api'
import {
  addRule,
  defaultAct,
  defaultCond,
  describeRule,
  MAX_RULES,
  moveRule,
  newPlace,
  newRoute,
  removeAttr,
  removePlace,
  removeRoute,
  renameAttr,
  renamePlace,
  renameRoute,
  reorderRules,
  updateRule,
} from './gameEdit'
import { fixture } from './test/fixture'

/** A game with a guard whose rules touch every kind of reference. */
function world(): GameFile {
  const g = fixture()
  g.places = [
    { name: 'plaza', x: 100, y: 100, r: 12 },
    { name: 'dock', x: 300, y: 50, r: 8 },
  ]
  g.routes = [{ name: 'ring', points: [[0, 0], [10, 10]], loop: true }]
  g.archetypes[0] = {
    ...g.archetypes[0]!,
    home: 'plaza',
    rules: [
      { when: { kind: 'attr_below', attr: 'health', value: 20 }, then: { kind: 'flee' } },
      { when: { kind: 'in_place', place: 'dock' }, then: { kind: 'say', text: 'hi', shout: false } },
      { when: { kind: 'always' }, then: { kind: 'goto', place: 'plaza' } },
      { when: { kind: 'always' }, then: { kind: 'patrol', route: 'ring' } },
    ],
  }
  return g
}

describe('rule list editing', () => {
  it('moves a rule and keeps the others in order', () => {
    const g = moveRule(world(), 0, 3, 0)
    expect(g.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['patrol', 'flee', 'say', 'goto'])
    const back = moveRule(g, 0, 0, 3)
    expect(back.archetypes[0]?.rules).toEqual(world().archetypes[0]?.rules)
  })
  it('clamps moves and ignores impossible ones', () => {
    const w = world()
    expect(moveRule(w, 0, 0, -5).archetypes[0]?.rules).toEqual(w.archetypes[0]?.rules)
    expect(moveRule(w, 0, 9, 0).archetypes[0]?.rules).toEqual(w.archetypes[0]?.rules)
    expect(moveRule(w, 0, 0, 99).archetypes[0]?.rules.at(-1)).toEqual(w.archetypes[0]?.rules[0])
  })
  it('reorders from a drag result, rejecting lists that are not permutations', () => {
    const w = world()
    expect(reorderRules(w, 0, [1, 0, 2, 3]).archetypes[0]?.rules[0]?.then.kind).toBe('say')
    expect(reorderRules(w, 0, [0, 0, 2, 3])).toEqual(w)
    expect(reorderRules(w, 0, [0, 1])).toEqual(w)
  })
  it('adds up to the limit the server enforces', () => {
    let g = fixture()
    for (let i = 0; i < MAX_RULES + 3; i++) g = addRule(g, 0)
    expect(g.archetypes[0]?.rules).toHaveLength(MAX_RULES)
  })
  it('updates one rule only', () => {
    const w = world()
    const g = updateRule(w, 0, 2, { when: { kind: 'chance', pct: 5 }, then: { kind: 'idle' } })
    expect(g.archetypes[0]?.rules[2]?.when).toEqual({ kind: 'chance', pct: 5 })
    expect(g.archetypes[0]?.rules[3]).toEqual(w.archetypes[0]?.rules[3])
  })
})

describe('references survive renames and removals', () => {
  it('renaming a place updates the home, conditions and go-to actions', () => {
    const g = renamePlace(world(), 0, 'square')
    const a = g.archetypes[0]!
    expect(a.home).toBe('square')
    expect(a.rules[2]?.then).toEqual({ kind: 'goto', place: 'square' })
    expect(a.rules[1]?.when).toEqual({ kind: 'in_place', place: 'dock' })
  })
  it('removing a place drops the rules that needed it and clears the home', () => {
    const g = removePlace(world(), 1)
    expect(g.places.map((p) => p.name)).toEqual(['plaza'])
    expect(g.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['flee', 'goto', 'patrol'])
    const h = removePlace(world(), 0)
    expect(h.archetypes[0]?.home).toBeNull()
    expect(h.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['flee', 'say', 'patrol'])
  })
  it('renaming and removing a route follows the patrol rules', () => {
    expect(renameRoute(world(), 0, 'loop road').archetypes[0]?.rules[3]?.then).toEqual({ kind: 'patrol', route: 'loop road' })
    const g = removeRoute(world(), 0)
    expect(g.routes).toEqual([])
    expect(g.archetypes[0]?.rules).toHaveLength(3)
  })
  it('renaming or deleting an attribute updates attribute conditions', () => {
    expect(renameAttr(world(), 0, 'hp').archetypes[0]?.rules[0]?.when).toMatchObject({ kind: 'attr_below', attr: 'hp' })
    expect(removeAttr(world(), 0).archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['say', 'goto', 'patrol'])
  })
  it('a no-op rename returns the same object', () => {
    const w = world()
    expect(renamePlace(w, 0, 'plaza')).toBe(w)
    expect(renamePlace(w, 9, 'x')).toBe(w)
  })
})

describe('new things', () => {
  it('place and route names are unique and positions are whole numbers', () => {
    const w = world()
    w.places.push(newPlace(w, 1.4, 2.6))
    expect(w.places.at(-1)).toMatchObject({ name: 'place', x: 1, y: 3 })
    expect(newPlace(w, 0, 0).name).toBe('place_2')
    expect(newRoute(w, [[1.2, 2.2], [3.7, 4]]).points).toEqual([[1, 2], [4, 4]])
  })
  it('defaults use whatever the game has, and are empty-safe', () => {
    const empty: GameFile = { ...fixture(), attrs: [], places: [], routes: [] }
    expect(defaultCond('attr_below', empty)).toMatchObject({ attr: '' })
    expect(defaultAct('patrol', empty)).toEqual({ kind: 'patrol', route: '' })
    expect(defaultCond('in_place', world())).toEqual({ kind: 'in_place', place: 'plaza' })
  })
  it('describes rules in a sentence', () => {
    const [a, b, c, d] = world().archetypes[0]!.rules
    expect(describeRule(a!)).toBe('When health is below 20: flee')
    expect(describeRule(b!)).toBe('When inside dock: say "hi"')
    expect(describeRule(c!)).toBe('Always: go to plaza')
    expect(describeRule(d!)).toBe('Always: patrol ring')
    expect(describeRule({ when: { kind: 'sees', who: 'any', within: 9 }, then: { kind: 'say', text: 'x', shout: true } })).toBe(
      'When it sees anyone within 9: shout "x"',
    )
  })
})
