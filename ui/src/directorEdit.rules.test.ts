import type { GameFile } from './api'
import { removeArch, removeWeather, renameArch, renameWeather } from './directorEdit'
import { describeRule, removePlace } from './gameEdit'
import { fixture } from './test/fixture'

/** A villager who flees raiders, shelters from rain, goes in at night and passes remarks. */
function village(): GameFile {
  const g = fixture()
  g.places = [{ name: 'home', x: 100, y: 100, r: 20 }]
  g.weather = [
    { name: 'rain', color: '#8899aa', intensity: 0.5, sight_mult: 1, drain: [] },
    { name: 'fog', color: '#8899aa', intensity: 0.5, sight_mult: 1, drain: [] },
  ]
  const base = g.archetypes[0]!
  g.archetypes = [
    {
      ...base,
      name: 'villager',
      home: 'home',
      rules: [
        { when: { kind: 'sees_kind', bot: 'raider', within: 30 }, then: { kind: 'flee' } },
        { when: { kind: 'weather', names: ['rain', 'fog'] }, then: { kind: 'indoors' } },
        { when: { kind: 'weather', names: ['rain'] }, then: { kind: 'indoors' } },
        { when: { kind: 'during', parts: ['dusk', 'night'] }, then: { kind: 'indoors' } },
        { when: { kind: 'always' }, then: { kind: 'chat', lines: ['Fine day.', 'Mind the wolves.'] } },
      ],
    },
    { ...base, name: 'raider', home: null, rules: [] },
  ]
  return g
}

describe('rules that name bot types and weather', () => {
  it('follow a renamed bot type and go with a removed one', () => {
    expect(renameArch(village(), 1, 'norseman').archetypes[0]!.rules[0]!.when).toEqual({ kind: 'sees_kind', bot: 'norseman', within: 30 })
    const gone = removeArch(village(), 1).archetypes[0]!.rules
    expect(gone.some((r) => r.when.kind === 'sees_kind')).toBe(false)
    expect(gone).toHaveLength(4)
  })

  it('follow renamed weather, and a rule whose list empties goes rather than meaning any weather', () => {
    expect(renameWeather(village(), 0, 'downpour').archetypes[0]!.rules[1]!.when).toEqual({ kind: 'weather', names: ['downpour', 'fog'] })
    const rules = removeWeather(village(), 0).archetypes[0]!.rules
    expect(rules.filter((r) => r.when.kind === 'weather')).toEqual([{ when: { kind: 'weather', names: ['fog'] }, then: { kind: 'indoors' } }])
  })

  it('drop going indoors when the home place goes', () => {
    const rules = removePlace(village(), 0).archetypes[0]!.rules
    expect(rules.some((r) => r.then.kind === 'indoors')).toBe(false)
  })

  it('read as sentences', () => {
    const r = village().archetypes[0]!.rules
    expect(r.map(describeRule)).toEqual([
      'When it sees a raider within 30: flee',
      'In rain or fog: go home, indoors',
      'In rain: go home, indoors',
      'At dusk or night: go home, indoors',
      'Always: remark "Fine day." (or 1 more)',
    ])
  })
})
