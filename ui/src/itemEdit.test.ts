import { describe, expect, it } from 'vitest'
import type { GameFile } from './api'
import { directorProblems, newItem, removeItem, renameItem } from './directorEdit'
import { removeAttr, renameAttr } from './gameEdit'

const base = (): GameFile => ({
  move_speed: 10,
  entity_radius: 1,
  interest_radius: 100,
  max_visible: 64,
  attrs: [{ name: 'health', min: 0, max: 100, default: 100, bits: 8, replicated: true, regen_per_sec: 0, on_zero: 'respawn' }],
  actions: [],
  archetypes: [{ name: 'drone', color: '#fff', count: 1, behaviour: 'wander', speed_mult: 1, sight: 10, attack: null, attrs: {}, home: null, rules: [], drops: [{ item: 'medkit', chance_pct: 50, min: 1, max: 2 }] }],
  places: [{ name: 'plaza', x: 1, y: 1, r: 5 }],
  routes: [],
  items: [{ name: 'medkit', color: '#ff0000', stack_max: 5, uses: [{ attr: 'health', delta: 40 }], consume: true }],
  events: [{ name: 'rain of loot', every_secs: 0, chance_pct: 100, steps: [{ kind: 'drop', item: 'medkit', count: 5, place: 'plaza' }] }],
})

describe('editing items keeps every reference honest', () => {
  it('renaming an item renames its drops and event steps', () => {
    const g = renameItem(base(), 0, 'bandage')
    expect(g.items![0]!.name).toBe('bandage')
    expect(g.archetypes[0]!.drops![0]!.item).toBe('bandage')
    expect(g.events![0]!.steps[0]).toMatchObject({ kind: 'drop', item: 'bandage' })
    expect(directorProblems(g)).toEqual([])
  })

  it('deleting an item removes its drops and the steps and events that used it', () => {
    const g = removeItem(base(), 0)
    expect(g.items).toEqual([])
    expect(g.archetypes[0]!.drops).toEqual([])
    expect(g.events).toEqual([])
  })

  it('renaming or deleting an attribute follows into what items do', () => {
    expect(renameAttr(base(), 0, 'hp').items![0]!.uses[0]!.attr).toBe('hp')
    expect(removeAttr(base(), 0).items![0]!.uses).toEqual([])
  })

  it('a deleted place drops the item step rather than widening it to everywhere', () => {
    const g = base()
    const gone = { ...g, places: [] }
    expect(directorProblems(gone).some((p) => p.includes('plaza'))).toBe(true)
  })

  it('new items get a free name', () => {
    expect(newItem(base()).name).toBe('item')
    expect(newItem({ ...base(), items: [{ ...base().items![0]!, name: 'item' }] }).name).toBe('item_2')
  })

  it('problems are reported for a drop of an item that is not there', () => {
    const g = base()
    g.archetypes[0]!.drops = [{ item: 'ghost', chance_pct: 10, min: 1, max: 1 }]
    expect(directorProblems(g).join(' ')).toContain('ghost')
  })
})
