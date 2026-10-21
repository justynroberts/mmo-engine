import { fixture } from './test/fixture'
import {
  addEvent,
  addWeather,
  addZone,
  removeZone,
  renameZone,
  defaultStep,
  describeStep,
  directorProblems,
  EVENT_PRESETS,
  removeArch,
  removeEvent,
  removeWeather,
  renameArch,
  renameEvent,
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

describe('weather zones', () => {
  const zoned = (): GameFile => ({
    ...withDirector(),
    zones: [{ name: 'haunt', place: 'plaza', weather: 'storm', active: true, height: 30 }],
    events: [...(withDirector().events ?? []), { name: 'lift', every_secs: 0, chance_pct: 100, steps: [{ kind: 'zone', zone: 'haunt', on: false }] }],
  })

  it('a zone over a known place with a known weather is valid, and a step switches it', () => {
    expect(directorProblems(zoned())).toEqual([])
    expect(describeStep({ kind: 'zone', zone: 'haunt', on: false })).toBe('lift the haunt')
  })

  it('renaming its place or weather follows through, deleting either takes the zone and the steps that switch it', () => {
    const g = zoned()
    expect(renamePlace(g, 0, 'square').zones![0]!.place).toBe('square')
    expect(renameWeather(g, 0, 'squall').zones![0]!.weather).toBe('squall')
    const noPlace = removePlace(g, 0)
    expect(noPlace.zones).toEqual([])
    expect(noPlace.events!.some((e) => e.name === 'lift')).toBe(false)
    expect(removeWeather(g, 0).zones).toEqual([])
  })

  it('renaming or removing the zone itself fixes its steps', () => {
    const g = zoned()
    expect(renameZone(g, 0, 'red mist').events!.find((e) => e.name === 'lift')!.steps[0]).toEqual({ kind: 'zone', zone: 'red mist', on: false })
    expect(removeZone(g, 0).events!.some((e) => e.name === 'lift')).toBe(false)
    expect(directorProblems({ ...g, zones: [{ ...g.zones![0]!, place: 'nowhere' }] })[0]).toMatch(/no place called “nowhere”/)
  })

  it('adds a zone only when there is a place and a weather to make it of', () => {
    expect(addZone(zoned()).zones).toHaveLength(2)
    expect(addZone({ ...fixture(), places: [] }).zones).toBeUndefined()
  })
})

describe('raids and local events', () => {
  const raid = (): GameFile => {
    const g = withDirector()
    return {
      ...g,
      items: [{ name: 'medkit', color: '#ff4d6d', stack_max: 5, uses: [], consume: true }],
      events: [
        {
          ...g.events![0]!,
          place: 'plaza',
          limit_secs: 300,
          steps: [{ kind: 'announce', text: 'To arms', banner: true, nearby: true }, { kind: 'defeat', within_secs: 60 }, { kind: 'reward', item: 'medkit', count: 1 }],
          fail_steps: [{ kind: 'announce', text: 'Fallen', banner: true }],
        },
        { name: 'aftermath', every_secs: 0, chance_pct: 100, when: { kind: 'after', event: 'raid', outcome: 'failure' }, steps: [{ kind: 'announce', text: 'Rebuild' }] },
      ],
    }
  }

  it('a raid with a place, a fight, a reward and a follow-up is valid', () => expect(directorProblems(raid())).toEqual([]))

  it('renaming an event moves what follows it; removing it stops that following', () => {
    const g = renameEvent(raid(), 0, 'siege')
    expect(g.events![1]!.when).toEqual({ kind: 'after', event: 'siege', outcome: 'failure' })
    const h = removeEvent(g, 0)
    expect(h.events!.map((e) => e.name)).toEqual(['aftermath'])
    expect(h.events![0]!.when).toBeNull()
  })

  it('rewards follow an item rename, in the fail steps too, and a deleted event place leaves nothing "nearby"', () => {
    const g = raid()
    g.events![0]!.fail_steps!.push({ kind: 'reward', item: 'medkit', count: 2, everyone: true })
    const r = rewriteRefs(g, 'item', 'medkit', 'bandage')
    expect(r.events![0]!.steps[2]).toMatchObject({ kind: 'reward', item: 'bandage' })
    expect(r.events![0]!.fail_steps![1]).toMatchObject({ kind: 'reward', item: 'bandage' })
    const p = removePlace(raid(), 0)
    expect(p.events![0]!.place).toBeNull()
    expect(p.events![0]!.steps[0]).toMatchObject({ kind: 'announce', nearby: false })
    expect(directorProblems(p)).toEqual([])
  })

  it('problems with the new parts are found in plain words', () => {
    const g = raid()
    g.events![0]!.place = null
    g.events![1]!.when = { kind: 'after', event: 'nothing' }
    const p = directorProblems(g)
    expect(p.some((x) => x.includes('needs the event to have a place'))).toBe(true)
    expect(p.some((x) => x.includes('which is not another event'))).toBe(true)
  })
})
