import { fixture } from './test/fixture'
import {
  addEvent,
  addWeather,
  defaultStep,
  describeStep,
  directorProblems,
  EVENT_PRESETS,
  removeArch,
  removeWeather,
  renameArch,
  renameWeather,
  rewriteRefs,
  STEP_KINDS,
  WEATHER_PRESETS,
} from './directorEdit'
import { removeAttr, removePlace, renameAttr, renamePlace } from './gameEdit'
import type { GameFile } from './api'

const withDirector = (): GameFile => ({
  ...fixture(),
  places: [{ name: 'plaza', x: 100, y: 100, r: 20 }],
  weather: [{ name: 'storm', color: '#445566', intensity: 0.8, sight_mult: 0.5, drain: [{ attr: 'health', per_sec: -1 }] }],
  events: [
    {
      name: 'raid',
      every_secs: 60,
      chance_pct: 100,
      steps: [
        { kind: 'announce', text: 'Raiders' },
        { kind: 'weather', name: 'storm', duration_secs: 30 },
        { kind: 'spawn', archetype: 'drone', count: 5, place: 'plaza' },
        { kind: 'hit', attr: 'health', delta: -10, place: 'plaza' },
        { kind: 'clear', archetype: 'drone' },
      ],
    },
  ],
})

describe('names stay consistent', () => {
  it('a valid file has no problems', () => expect(directorProblems(withDirector())).toEqual([]))

  it('renaming an attribute, bot type, place or weather follows through', () => {
    let g = renameAttr(withDirector(), 0, 'hp')
    expect(g.weather![0]!.drain[0]!.attr).toBe('hp')
    expect(g.events![0]!.steps[3]).toMatchObject({ attr: 'hp' })
    g = renameArch(g, 0, 'wasp')
    expect(g.events![0]!.steps[2]).toMatchObject({ archetype: 'wasp' })
    expect(g.events![0]!.steps[4]).toMatchObject({ archetype: 'wasp' })
    g = renamePlace(g, 0, 'square')
    expect(g.events![0]!.steps[2]).toMatchObject({ place: 'square' })
    expect(g.events![0]!.steps[3]).toMatchObject({ place: 'square' })
    g = renameWeather(g, 0, 'hail')
    expect(g.events![0]!.steps[1]).toMatchObject({ name: 'hail' })
    expect(directorProblems(g)).toEqual([])
  })

  it('deleting something drops what depended on it, and never widens a hit to everywhere', () => {
    let g = removePlace(withDirector(), 0)
    expect(g.events![0]!.steps.map((s) => s.kind)).toEqual(['announce', 'weather', 'clear'])
    g = removeArch(withDirector(), 0)
    expect(g.events![0]!.steps.map((s) => s.kind)).toEqual(['announce', 'weather', 'hit'])
    g = removeAttr(withDirector(), 0)
    expect(g.weather![0]!.drain).toEqual([])
    expect(g.events![0]!.steps.some((s) => s.kind === 'hit')).toBe(false)
    g = removeWeather(withDirector(), 0)
    expect(g.events![0]!.steps.some((s) => s.kind === 'weather')).toBe(false)
  })

  it('an event with nothing left is removed rather than kept empty', () => {
    const g = rewriteRefs({ ...withDirector(), events: [{ name: 'only', every_secs: 0, chance_pct: 100, steps: [{ kind: 'clear', archetype: 'drone' }] }] }, 'arch', 'drone', null)
    expect(g.events).toEqual([])
  })

  it('files with no weather or events are returned untouched', () => {
    const g = fixture()
    expect(renameAttr(g, 0, 'hp').weather).toBeUndefined()
  })
})

describe('starting points', () => {
  it('every preset is valid against a normal game and against an empty one', () => {
    const full = withDirector()
    const bare: GameFile = { ...fixture(), archetypes: [], places: [], actions: [] }
    for (const g of [full, bare]) {
      for (const p of WEATHER_PRESETS) expect(directorProblems(addWeather(g, p.id))).toEqual([])
      for (const p of EVENT_PRESETS) expect(directorProblems(addEvent(addWeather(g, 'fog'), p.id))).toEqual([])
    }
  })

  it('names stay unique when you add the same preset twice', () => {
    let g = fixture()
    g = addEvent(addEvent(g, 'wave'), 'wave')
    g = addWeather(addWeather(g, 'fog'), 'fog')
    expect(new Set(g.events!.map((e) => e.name)).size).toBe(2)
    expect(new Set(g.weather!.map((w) => w.name)).size).toBe(2)
  })

  it('every kind of step can be created and described', () => {
    for (const k of STEP_KINDS) expect(describeStep(defaultStep(k.kind, withDirector()))).not.toBe('')
  })

  it('problems are found in plain words', () => {
    const g = withDirector()
    g.events![0]!.steps[2] = { kind: 'spawn', archetype: 'ghost', count: 1, place: 'atlantis' }
    const p = directorProblems(g)
    expect(p.join(' ')).toContain('ghost')
    expect(p.join(' ')).toContain('atlantis')
  })
})
