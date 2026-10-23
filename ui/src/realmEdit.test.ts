import { describe, expect, it } from 'vitest'
import type { GameFile } from './api'
import { removeArch, removeItem, renameArch, renameItem } from './directorEdit'
import { removePlace, renamePlace } from './gameEdit'

function game(): GameFile {
  return {
    move_speed: 20,
    entity_radius: 0.5,
    interest_radius: 60,
    max_visible: 64,
    attrs: [{ name: 'health', min: 0, max: 100, default: 100, bits: 8, replicated: true, regen_per_sec: 0, on_zero: 'respawn' }],
    actions: [],
    archetypes: [{ name: 'ghoul', color: '#888888', count: 0, behaviour: 'wander', speed_mult: 1, sight: 40, attack: null, attrs: {}, home: null, rules: [], loot: 'crypt' }],
    places: [{ name: 'forge', x: 10, y: 10, r: 5 }, { name: 'crypt', x: 50, y: 50, r: 10 }],
    routes: [],
    items: [
      { name: 'ore', color: '#888888', stack_max: 9, uses: [] },
      { name: 'blade', color: '#888888', stack_max: 1, uses: [], rarity: 'epic' },
      { name: 'seed', color: '#888888', stack_max: 9, uses: [] },
    ],
    loot: [{ name: 'crypt', entries: [{ item: 'blade' }] }],
    recipes: [{ name: 'blade', needs: [{ item: 'ore', count: 3 }], makes: 'blade', near: 'forge' }],
    crops: [{ name: 'turnip', seed: 'seed', yields: 'ore' }],
    dungeons: [{ name: 'The Crypt', place: 'crypt', bots: [{ bot: 'ghoul', count: 3 }], reward: { item: 'blade' } }],
  } as unknown as GameFile
}

describe('realm references', () => {
  it('follow a renamed item everywhere', () => {
    const g = renameItem(game(), 1, 'ember blade')
    expect(g.loot![0]!.entries[0]!.item).toBe('ember blade')
    expect(g.recipes![0]!.makes).toBe('ember blade')
    expect(g.dungeons![0]!.reward!.item).toBe('ember blade')
  })

  it('drop what a deleted item breaks, and an emptied loot table with its pointer', () => {
    let g = removeItem(game(), 0) // ore: the recipe's ingredient and the crop's harvest
    expect(g.recipes).toEqual([])
    expect(g.crops).toEqual([])
    g = removeItem(g, 0) // now the blade
    expect(g.loot).toEqual([])
    expect(g.archetypes[0]!.loot).toBeUndefined()
    expect(g.dungeons![0]!.reward).toBeUndefined()
  })

  it('follow bot types and places, and a deleted place takes its recipes and dungeon with it', () => {
    let g = renameArch(game(), 0, 'wight')
    expect(g.dungeons![0]!.bots[0]!.bot).toBe('wight')
    g = renamePlace(g, 0, 'smithy')
    expect(g.recipes![0]!.near).toBe('smithy')
    g = removePlace(g, 1)
    expect(g.dungeons).toEqual([])
    g = removePlace(g, 0)
    expect(g.recipes).toEqual([])
    expect(removeArch(game(), 0).dungeons).toEqual([])
  })
})
