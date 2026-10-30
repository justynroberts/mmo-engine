import type { GameFile } from './api'
import { removeArch, renameArch, rewriteRefs } from './directorEdit'
import { addPhase, bossProblem, newBoss, removePhase } from './bossEdit'
import { removeAction, renameAction } from './gameEdit'
import { fixture } from './test/fixture'

/** A boss that slams with the game's first action and calls in raiders at half health. */
function lair(): GameFile {
  const g = fixture()
  const base = g.archetypes[0]!
  const act = g.actions[0]!.name
  g.archetypes = [
    {
      ...base,
      name: 'warlord',
      boss: {
        phases: [
          { below: 100, moves: [{ action: act, every_secs: 6, warn_secs: 2, at: 'random' }] },
          { below: 50, say: 'To me!', moves: [{ action: act, every_secs: 4 }], adds: [{ bot: 'raider', count: 3 }] },
        ],
      },
    },
    { ...base, name: 'raider', rules: [] },
  ]
  return g
}

describe('a boss names actions and bot types', () => {
  it('follows a renamed action and drops the moves of a removed one', () => {
    const g = lair()
    const renamed = renameAction(g, 0, 'slam').archetypes[0]!.boss!
    expect(renamed.phases.flatMap((p) => p.moves.map((m) => m.action))).toEqual(['slam', 'slam'])
    const gone = removeAction(g, 0).archetypes[0]!.boss!
    expect(gone.phases.map((p) => p.moves.length)).toEqual([0, 0])
  })

  it('follows a renamed add and drops a removed one', () => {
    expect(renameArch(lair(), 1, 'reaver').archetypes[0]!.boss!.phases[1]!.adds).toEqual([{ bot: 'reaver', count: 3 }])
    const gone = removeArch(lair(), 1).archetypes[0]!.boss!.phases[1]!
    expect(gone.adds).toBeUndefined()
    expect(gone.say).toBe('To me!')
  })
})

describe('editing a boss', () => {
  it('starts with one phase using its attack, adds halving phases, and keeps the first', () => {
    const g = lair()
    let b = newBoss(g, g.actions[0]!.name)
    expect(bossProblem(b, g)).toBeNull()
    b = addPhase(addPhase(b))
    expect(b.phases.map((p) => p.below)).toEqual([100, 50, 25])
    expect(removePhase(b, 0).phases).toHaveLength(3)
    expect(removePhase(b, 1).phases.map((p) => p.below)).toEqual([100, 25])
  })

  it('says what the server would refuse', () => {
    const g = lair()
    const b = g.archetypes[0]!.boss!
    expect(bossProblem(b, g)).toBeNull()
    expect(bossProblem({ ...b, phases: [b.phases[0]!, { ...b.phases[1]!, below: 100 }] }, g)).toMatch(/below the phase before/)
    expect(bossProblem({ ...b, phases: [{ below: 100, moves: [{ action: 'nuke', every_secs: 3 }] }] }, g)).toMatch(/no action called “nuke”/)
    expect(bossProblem({ ...b, phases: [{ below: 100, moves: [], adds: [{ bot: 'warlord', count: 1 }] }] }, g)).toMatch(/cannot be a boss/)
  })
})

describe('the start place', () => {
  it('follows a renamed place and goes with a removed one', () => {
    const g: GameFile = { ...fixture(), places: [{ name: 'town', x: 10, y: 10, r: 20 }], start_place: 'town' }
    expect(rewriteRefs(g, 'place', 'town', 'city').start_place).toBe('city')
    expect(rewriteRefs(g, 'place', 'town', null).start_place).toBeUndefined()
  })
})
