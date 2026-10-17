import { fixture } from './test/fixture'
import {
  needsReset,
  newAction,
  newArchetype,
  newAttr,
  parseNum,
  removeAction,
  removeAttr,
  renameAction,
  renameAttr,
  uniqueName,
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
})

describe('parseNum', () => {
  it('accepts numbers and rejects half-typed input', () => {
    expect(parseNum('12.5')).toBe(12.5)
    expect(parseNum('-3')).toBe(-3)
    expect(parseNum('1e2')).toBe(100)
    for (const bad of ['', ' ', '-', '.', 'abc', '1,5', 'NaN', 'Infinity']) expect(parseNum(bad)).toBeNull()
  })
})
