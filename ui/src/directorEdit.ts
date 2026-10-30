// Editing weather and events. They refer to attributes, bot types, places and each other by name, so renaming or
// deleting any of those has to rewrite them, the same way the bot rules are kept honest in gameEdit.ts.

import type { Archetype, BossPhase, EventDef, GameFile, ItemDef, Step, WeatherDef, WeatherZone } from './api'
import { classProblems, questProblems, rewriteQuestRefs, uniqueName } from './gameEdit'
import { rewriteRealmRefs } from './realmEdit'

export type RefKind = 'attr' | 'arch' | 'place' | 'weather' | 'item'

/** Rewrite every weather and event step that mentions `from`. With `to` null the references are removed: a drain,
 * a step, and an event left with no steps. (A deleted place drops its steps rather than widening them to "everywhere".) */
export function rewriteRefs(g: GameFile, kind: RefKind, from: string, to: string | null): GameFile {
  if (!g.weather && !g.events && !g.zones && !g.quests && kind !== 'item' && kind !== 'arch' && kind !== 'place') return g
  const weather = (g.weather ?? []).map((w) =>
    kind === 'attr'
      ? { ...w, drain: w.drain.flatMap((d) => (d.attr !== from ? [d] : to === null ? [] : [{ ...d, attr: to }])) }
      : w,
  )
  const fixStep = (s: Step): Step | null => {
    switch (s.kind) {
      case 'announce':
        return s
      case 'weather':
        if (kind !== 'weather' || s.name !== from) return s
        return to === null ? null : { ...s, name: to }
      case 'spawn':
        if (kind === 'arch' && s.archetype === from) return to === null ? null : { ...s, archetype: to }
        if (kind === 'place' && s.place === from) return to === null ? null : { ...s, place: to }
        return s
      case 'hit':
        if (kind === 'attr' && s.attr === from) return to === null ? null : { ...s, attr: to }
        if (kind === 'place' && s.place === from) return to === null ? null : { ...s, place: to }
        return s
      case 'clear':
        if (kind === 'arch' && s.archetype === from) return to === null ? null : { ...s, archetype: to }
        return s
      case 'drop':
        if (kind === 'item' && s.item === from) return to === null ? null : { ...s, item: to }
        if (kind === 'place' && s.place === from) return to === null ? null : { ...s, place: to }
        return s
      case 'zone':
      case 'wait':
      case 'defeat':
      case 'gather':
      case 'plague':
        return s
      case 'reward':
        if (kind === 'item' && s.item === from) return to === null ? null : { ...s, item: to }
        return s
    }
  }
  // A zone over a place or of a weather follows a rename and goes when either does (and the steps that switch it).
  const zones: WeatherZone[] = (g.zones ?? []).flatMap((z) => {
    const field = kind === 'place' ? 'place' : kind === 'weather' ? 'weather' : null
    if (!field || z[field] !== from) return [z]
    return to === null ? [] : [{ ...z, [field]: to }]
  })
  const goneZones = new Set((g.zones ?? []).filter((z) => !zones.some((k) => k.name === z.name)).map((z) => z.name))
  const fixSteps = (list: Step[]) => list.flatMap((s) => (s.kind === 'zone' && goneZones.has(s.zone) ? [] : (fixStep(s) ?? [])))
  const events = (g.events ?? [])
    .map((e) => {
      const out: EventDef = { ...e, steps: fixSteps(e.steps), ...(e.fail_steps ? { fail_steps: fixSteps(e.fail_steps) } : {}) }
      // An event's own place follows a rename; deleted, the event happens anywhere (and nothing is "nearby").
      if (kind === 'place' && e.place === from) {
        out.place = to
        if (to === null) out.steps = out.steps.map((s) => (s.kind === 'announce' ? { ...s, nearby: false } : s))
      }
      return out
    })
    .filter((e) => e.steps.length > 0)
  // Rules name bot types (sees_kind) and weather; a rule left pointing at nothing goes (an emptied weather list
  // would mean any weather, so it goes too).
  const ruled: Archetype[] =
    kind === 'arch' || kind === 'weather'
      ? g.archetypes.map((a) => ({
          ...a,
          rules: a.rules.flatMap((r) => {
            const w = r.when
            if (kind === 'arch' && w.kind === 'sees_kind' && w.bot === from) return to === null ? [] : [{ ...r, when: { ...w, bot: to } }]
            if (kind === 'weather' && w.kind === 'weather' && w.names.includes(from)) {
              const names = to === null ? w.names.filter((n) => n !== from) : w.names.map((n) => (n === from ? to : n))
              return names.length === 0 ? [] : [{ ...r, when: { ...w, names } }]
            }
            return [r]
          }),
        }))
      : g.archetypes
  // A boss's adds name bot types: renamed with them, dropped with them.
  const bossed: Archetype[] =
    kind === 'arch'
      ? ruled.map((a) => {
          if (!a.boss?.phases.some((p) => p.adds?.some((x) => x.bot === from))) return a
          const phases = a.boss.phases.map((p) => {
            const adds = (p.adds ?? []).flatMap((x) => (x.bot !== from ? [x] : to === null ? [] : [{ ...x, bot: to }]))
            const out: BossPhase = { ...p, adds }
            if (adds.length === 0) delete out.adds
            return out
          })
          return { ...a, boss: { ...a.boss, phases } }
        })
      : ruled
  // Loot tables and shops name items too, and so does the currency.
  const archetypes: Archetype[] =
    kind === 'item'
      ? bossed.map((a) => {
          if (!a.drops && !a.shop) return a
          const out = { ...a }
          if (a.drops) out.drops = a.drops.flatMap((d) => (d.item !== from ? [d] : to === null ? [] : [{ ...d, item: to }]))
          if (a.shop) {
            const shop = a.shop.flatMap((s) => (s.item !== from ? [s] : to === null ? [] : [{ ...s, item: to }]))
            if (shop.length > 0) out.shop = shop
            else delete out.shop
          }
          return out
        })
      : bossed
  // Quests name bot types, places and items (and each other, handled by renameQuest/removeQuest).
  const quests = g.quests ? { quests: rewriteQuestRefs(g.quests, kind, from, to) } : {}
  const next: GameFile = { ...g, archetypes, weather, events, ...(g.zones ? { zones } : {}), ...quests }
  // Where players come into the world follows a renamed place; with it gone, they come in anywhere.
  if (kind === 'place' && g.start_place === from) {
    if (to === null) delete next.start_place
    else next.start_place = to
  }
  if (kind === 'item' && g.currency === from) {
    if (to === null) delete next.currency
    else next.currency = to
  }
  return rewriteRealmRefs(next, kind, from, to)
}

export const MAX_ZONES = 32

/** A new weather zone over the first place, with the first weather (or nothing, if the game has neither yet). */
export function addZone(g: GameFile): GameFile {
  const list = g.zones ?? []
  if (list.length >= MAX_ZONES || !g.places[0] || !g.weather?.[0]) return g
  const name = uniqueName('zone', list.map((z) => z.name))
  return { ...g, zones: [...list, { name, place: g.places[0].name, weather: g.weather[0].name, active: true, height: 30 }] }
}

/** Rename a zone, and the steps that switch it. */
export function renameZone(g: GameFile, index: number, name: string): GameFile {
  const old = g.zones?.[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    zones: (g.zones ?? []).map((z, i) => (i === index ? { ...z, name } : z)),
    events: (g.events ?? []).map((e) => ({ ...e, steps: e.steps.map((s) => (s.kind === 'zone' && s.zone === old ? { ...s, zone: name } : s)) })),
  }
}

/** Remove a zone, and the steps that switch it (an event left with none goes too). */
export function removeZone(g: GameFile, index: number): GameFile {
  const gone = g.zones?.[index]?.name
  if (gone === undefined) return g
  return {
    ...g,
    zones: (g.zones ?? []).filter((_, i) => i !== index),
    events: (g.events ?? []).map((e) => ({ ...e, steps: e.steps.filter((s) => !(s.kind === 'zone' && s.zone === gone)) })).filter((e) => e.steps.length > 0),
  }
}

export const MAX_ITEMS = 32

export function newItem(g: GameFile): ItemDef {
  return { name: uniqueName('item', (g.items ?? []).map((i) => i.name)), color: '#f5d90a', stack_max: 5, uses: [], consume: true }
}
export function renameItem(g: GameFile, index: number, name: string): GameFile {
  const old = g.items?.[index]?.name
  if (old === undefined || old === name) return g
  return rewriteRefs({ ...g, items: (g.items ?? []).map((x, i) => (i === index ? { ...x, name } : x)) }, 'item', old, name)
}
export function removeItem(g: GameFile, index: number): GameFile {
  const gone = g.items?.[index]?.name
  if (gone === undefined) return g
  return rewriteRefs({ ...g, items: (g.items ?? []).filter((_, i) => i !== index) }, 'item', gone, null)
}

export function renameArch(g: GameFile, index: number, name: string): GameFile {
  const old = g.archetypes[index]?.name
  if (old === undefined || old === name) return g
  return rewriteRefs({ ...g, archetypes: g.archetypes.map((a, i) => (i === index ? { ...a, name } : a)) }, 'arch', old, name)
}
export function removeArch(g: GameFile, index: number): GameFile {
  const gone = g.archetypes[index]?.name
  if (gone === undefined) return g
  return rewriteRefs({ ...g, archetypes: g.archetypes.filter((_, i) => i !== index) }, 'arch', gone, null)
}

export function renameWeather(g: GameFile, index: number, name: string): GameFile {
  const old = g.weather?.[index]?.name
  if (old === undefined || old === name) return g
  return rewriteRefs({ ...g, weather: (g.weather ?? []).map((w, i) => (i === index ? { ...w, name } : w)) }, 'weather', old, name)
}
export function removeWeather(g: GameFile, index: number): GameFile {
  const gone = g.weather?.[index]?.name
  if (gone === undefined) return g
  return rewriteRefs({ ...g, weather: (g.weather ?? []).filter((_, i) => i !== index) }, 'weather', gone, null)
}

export const MAX_WEATHER = 16
export const MAX_EVENTS = 32
export const MAX_STEPS = 24

/** Rename an event, and what starts after it. */
export function renameEvent(g: GameFile, index: number, name: string): GameFile {
  const old = g.events?.[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    events: (g.events ?? []).map((e, i) => {
      const n = i === index ? { ...e, name } : e
      return n.when?.kind === 'after' && n.when.event === old ? { ...n, when: { ...n.when, event: name } } : n
    }),
  }
}

/** Remove an event; what started after it no longer starts on its own that way. */
export function removeEvent(g: GameFile, index: number): GameFile {
  const gone = g.events?.[index]?.name
  if (gone === undefined) return g
  return {
    ...g,
    events: (g.events ?? []).filter((_, i) => i !== index).map((e) => (e.when?.kind === 'after' && e.when.event === gone ? { ...e, when: null } : e)),
  }
}

// ---------------------------------------------------------------- starting points

export interface Preset<T> {
  id: string
  label: string
  make: (g: GameFile) => T
}

const healthName = (g: GameFile) => (g.attrs.find((a) => a.name === 'health') ?? g.attrs[0])?.name

export const WEATHER_PRESETS: readonly Preset<WeatherDef>[] = [
  { id: 'fog', label: 'Fog: bots see half as far', make: () => ({ name: 'fog', color: '#8fa3b5', intensity: 0.8, sight_mult: 0.5, drain: [] }) },
  { id: 'rain', label: 'Rain: just for looks', make: () => ({ name: 'rain', color: '#5b7fa6', intensity: 0.5, sight_mult: 0.8, drain: [] }) },
  {
    id: 'acid',
    label: 'Acid rain: slowly hurts everyone',
    make: (g) => ({ name: 'acid rain', color: '#9bd21c', intensity: 0.7, sight_mult: 0.7, drain: healthName(g) ? [{ attr: healthName(g)!, per_sec: -1 }] : [] }),
  },
  {
    id: 'clear',
    label: 'Clear night: bots see further',
    make: () => ({ name: 'clear night', color: '#1a2a55', intensity: 0.2, sight_mult: 1.5, drain: [] }),
  },
  { id: 'red', label: 'Red mist: spooky, bots half blind (for a zone)', make: () => ({ name: 'red mist', color: '#b3121b', intensity: 0.9, sight_mult: 0.5, drain: [] }) },
  { id: 'green', label: 'Green mist: thick enough to hide a castle (for a zone)', make: () => ({ name: 'green mist', color: '#3fbf4a', intensity: 1, sight_mult: 0.4, drain: [] }) },
  {
    id: 'healing',
    label: 'Healing light: mends whoever stands in it (for a zone)',
    make: (g) => ({ name: 'healing light', color: '#ffd86b', intensity: 0.35, sight_mult: 1, drain: healthName(g) ? [{ attr: healthName(g)!, per_sec: 5 }] : [] }),
  },
]

export const EVENT_PRESETS: readonly Preset<EventDef>[] = [
  {
    id: 'announce',
    label: 'Announcement: a message to everyone',
    make: () => ({ name: 'announcement', every_secs: 0, chance_pct: 100, steps: [{ kind: 'announce', text: 'Something is happening.' }] }),
  },
  {
    id: 'wave',
    label: 'Bot wave: a group arrives',
    make: (g) => ({
      name: 'wave',
      every_secs: 0,
      chance_pct: 100,
      steps: [
        { kind: 'announce', text: 'Intruders spotted.' },
        ...(g.archetypes[0] ? [{ kind: 'spawn', archetype: g.archetypes[0].name, count: 10, place: g.places[0]?.name ?? null } as Step] : []),
      ],
    }),
  },
  {
    id: 'strike',
    label: 'Strike: hurts everyone in a place',
    make: (g) => ({
      name: 'strike',
      every_secs: 0,
      chance_pct: 100,
      steps: [
        { kind: 'announce', text: 'Incoming.' },
        ...(healthName(g) ? [{ kind: 'hit', attr: healthName(g)!, delta: -30, place: g.places[0]?.name ?? null } as Step] : []),
      ],
    }),
  },
  {
    id: 'storm',
    label: 'Storm every few minutes (random)',
    make: (g) => ({
      name: 'storm front',
      every_secs: 300,
      chance_pct: 50,
      steps: [{ kind: 'announce', text: 'A storm is rolling in.' }, ...(g.weather?.[0] ? [{ kind: 'weather', name: g.weather[0].name, duration_secs: 90 } as Step] : [])],
    }),
  },
  {
    id: 'raid',
    label: 'Raid: warn a place, send waves, beat them in time or it falls',
    make: (g) => {
      const place = g.places[0]?.name ?? null
      const foe = g.archetypes[0]?.name
      const reward = g.items?.[0]?.name
      return {
        name: 'raid',
        every_secs: 0,
        chance_pct: 100,
        place,
        limit_secs: 600,
        steps: [
          { kind: 'announce', text: `Raiders are marching on ${place ?? 'us'}!`, banner: true },
          { kind: 'wait', secs: 30 },
          ...(foe ? ([{ kind: 'spawn', archetype: foe, count: 8, place: null }, { kind: 'wait', secs: 45 }, { kind: 'spawn', archetype: foe, count: 12, place: null }, { kind: 'defeat', within_secs: 240 }] as Step[]) : []),
          { kind: 'announce', text: `${place ?? 'The town'} holds. The raiders are beaten.`, banner: true },
          ...(reward ? [{ kind: 'reward', item: reward, count: 2 } as Step] : []),
        ],
        fail_steps: [{ kind: 'announce', text: `${place ?? 'The town'} has fallen to the raiders.`, banner: true }],
      }
    },
  },
  {
    id: 'boss',
    label: 'World boss: a named foe for everyone, a reward for whoever is there',
    make: (g) => ({
      name: 'world boss',
      every_secs: 0,
      chance_pct: 100,
      place: g.places[0]?.name ?? null,
      steps: [
        { kind: 'announce', text: 'The ground shakes. Something old has woken.', banner: true },
        ...(g.archetypes[0] ? ([{ kind: 'spawn', archetype: g.archetypes[0].name, count: 1, place: null, name: 'The Hollow King' }, { kind: 'defeat', within_secs: 900 }] as Step[]) : []),
        { kind: 'announce', text: 'The Hollow King has fallen!', banner: true },
        ...(g.items?.[0] ? [{ kind: 'reward', item: g.items[0].name, count: 3 } as Step] : []),
      ],
      fail_steps: [{ kind: 'announce', text: 'The Hollow King returns to the dark, for now.', banner: true }],
    }),
  },
  {
    id: 'night',
    label: 'Night terror: at nightfall, things come out',
    make: (g) => ({
      name: 'night terror',
      every_secs: 0,
      chance_pct: 100,
      when: { kind: 'time', at: 'dusk' },
      steps: [
        { kind: 'announce', text: 'Night falls. Stay near the light.', banner: true },
        ...(g.archetypes[0] ? [{ kind: 'spawn', archetype: g.archetypes[0].name, count: 15, place: null } as Step] : []),
      ],
    }),
  },
  {
    id: 'defend',
    label: 'Defend: starts when players gather at a place',
    make: (g) => ({
      name: 'defend',
      every_secs: 0,
      chance_pct: 100,
      place: g.places[0]?.name ?? null,
      when: { kind: 'players', count: 2 },
      cooldown_secs: 1800,
      steps: [
        { kind: 'announce', text: 'You are not alone here. Hold your ground!', banner: true, nearby: !!g.places[0] },
        ...(g.archetypes[0] ? ([{ kind: 'spawn', archetype: g.archetypes[0].name, count: 6, place: null }, { kind: 'defeat', within_secs: 180 }] as Step[]) : []),
        ...(g.items?.[0] ? [{ kind: 'reward', item: g.items[0].name, count: 1 } as Step] : []),
      ],
    }),
  },
  {
    id: 'cleanup',
    label: 'Cleanup: remove every bot of one type',
    make: (g) => ({ name: 'cleanup', every_secs: 0, chance_pct: 100, steps: g.archetypes[0] ? [{ kind: 'clear', archetype: g.archetypes[0].name }] : [{ kind: 'announce', text: 'All clear.' }] }),
  },
]

export function addWeather(g: GameFile, id?: string): GameFile {
  const list = g.weather ?? []
  if (list.length >= MAX_WEATHER) return g
  const made = (WEATHER_PRESETS.find((p) => p.id === id) ?? WEATHER_PRESETS[0]!).make(g)
  const w: WeatherDef = id ? made : { name: 'weather', color: '#8899aa', intensity: 0.5, sight_mult: 1, drain: [] }
  return { ...g, weather: [...list, { ...w, name: uniqueName(w.name, list.map((x) => x.name)) }] }
}

export function addEvent(g: GameFile, id?: string): GameFile {
  const list = g.events ?? []
  if (list.length >= MAX_EVENTS) return g
  const made = (EVENT_PRESETS.find((p) => p.id === id) ?? EVENT_PRESETS[0]!).make(g)
  return { ...g, events: [...list, { ...made, name: uniqueName(made.name, list.map((x) => x.name)) }] }
}

export const STEP_KINDS: readonly { kind: Step['kind']; label: string }[] = [
  { kind: 'announce', label: 'Announce' },
  { kind: 'weather', label: 'Set weather' },
  { kind: 'spawn', label: 'Spawn bots' },
  { kind: 'hit', label: 'Change an attribute' },
  { kind: 'clear', label: 'Remove bots' },
  { kind: 'drop', label: 'Drop items' },
  { kind: 'zone', label: 'Switch a weather zone' },
  { kind: 'wait', label: 'Wait' },
  { kind: 'defeat', label: 'Wait until its bots are beaten' },
  { kind: 'reward', label: 'Reward the players' },
  { kind: 'gather', label: 'Gather the town' },
  { kind: 'plague', label: 'Bring the sickness' },
]

/** Talk occasions a gathering may use (proto::talk). */
export const GATHER_OCCASIONS = ['market', 'feast', 'wedding', 'cheer', 'mourn'] as const

/** A new step of a kind, filled with things that exist (null where the game has none to offer). */
export function defaultStep(kind: Step['kind'], g: GameFile): Step {
  switch (kind) {
    case 'announce':
      return { kind, text: 'Attention, everyone.' }
    case 'weather':
      return { kind, name: g.weather?.[0]?.name ?? null, duration_secs: 120 }
    case 'spawn':
      return { kind, archetype: g.archetypes[0]?.name ?? '', count: 5, place: g.places[0]?.name ?? null }
    case 'hit':
      return { kind, attr: healthName(g) ?? '', delta: -10, place: null }
    case 'clear':
      return { kind, archetype: g.archetypes[0]?.name ?? '' }
    case 'drop':
      return { kind, item: g.items?.[0]?.name ?? '', count: 10, place: g.places[0]?.name ?? null }
    case 'zone':
      return { kind, zone: g.zones?.[0]?.name ?? '', on: true }
    case 'wait':
      return { kind, secs: 30 }
    case 'defeat':
      return { kind, within_secs: 300 }
    case 'reward':
      return { kind, item: g.items?.[0]?.name ?? '', count: 1 }
    case 'gather':
      return { kind, secs: 600, occasion: 'market' }
    case 'plague':
      return { kind, secs: 1500, spread_pct: 12, deaths_pct: 20, first: 3 }
  }
}

export function describeStep(s: Step): string {
  switch (s.kind) {
    case 'announce':
      return `${s.banner ? 'banner' : 'say'} “${s.text}”${s.nearby ? ' to those nearby' : ''}`
    case 'weather':
      return s.name ? `weather ${s.name} for ${s.duration_secs}s` : 'clear the weather'
    case 'spawn':
      return `${s.count} ${s.name ? `${s.name} (${s.archetype})` : s.archetype}${s.place ? ` at ${s.place}` : ''}`
    case 'hit':
      return `${s.attr} ${s.delta > 0 ? '+' : ''}${s.delta}${s.place ? ` at ${s.place}` : ' everywhere'}`
    case 'clear':
      return `remove every ${s.archetype}`
    case 'drop':
      return `${s.count} ${s.item} on the ground${s.place ? ` at ${s.place}` : ''}`
    case 'zone':
      return `${s.on ? 'raise' : 'lift'} the ${s.zone}`
    case 'wait':
      return `wait ${s.secs}s`
    case 'defeat':
      return `wait until they are beaten${s.within_secs ? ` (within ${s.within_secs}s or it fails)` : ''}`
    case 'reward':
      return `give ${s.count} ${s.item} to ${s.everyone ? 'everyone online' : 'those who took part'}`
    case 'gather':
      return `the town out on its square for ${s.secs}s (${s.occasion}${s.dance ? ', dancing' : ''})`
    case 'plague':
      return `the sickness in the town for ${s.secs}s (${s.first} fall ill, ${s.spread_pct}% a minute to those close, ${s.deaths_pct}% die)`
  }
}

/** What stops this game file being valid for the server, in words (the server checks the same things). */
export function directorProblems(g: GameFile): string[] {
  const out: string[] = []
  const attr = new Set(g.attrs.map((a) => a.name))
  const arch = new Set(g.archetypes.map((a) => a.name))
  const place = new Set(g.places.map((p) => p.name))
  const wea = new Set((g.weather ?? []).map((w) => w.name))
  const itm = new Set((g.items ?? []).map((i) => i.name))
  for (const a of g.archetypes) for (const d of a.drops ?? []) if (!itm.has(d.item)) out.push(`${a.name}: drops “${d.item}”, which is not an item`)
  for (const a of g.archetypes) for (const s of a.shop ?? []) if (!itm.has(s.item)) out.push(`${a.name}: sells “${s.item}”, which is not an item`)
  if (g.currency !== undefined && !itm.has(g.currency)) out.push(`the currency “${g.currency}” is not an item`)
  const evs = new Set((g.events ?? []).map((e) => e.name))
  for (const e of g.events ?? []) {
    if (e.place && !place.has(e.place)) out.push(`${e.name}: no place called “${e.place}”`)
    if (e.when?.kind === 'after' && (e.when.event === e.name || !evs.has(e.when.event))) out.push(`${e.name}: starts after “${e.when.event}”, which is not another event`)
    const all = [...e.steps.map((s, i) => [s, `${e.name} step ${i + 1}`] as const), ...(e.fail_steps ?? []).map((s, i) => [s, `${e.name} if it fails, step ${i + 1}`] as const)]
    all.forEach(([s, at]) => {
      if (s.kind === 'announce' && s.nearby && !e.place && !e.roams) out.push(`${at}: “to those nearby” needs the event to have a place`)
      if (s.kind === 'gather' && !e.place && !e.roams) out.push(`${at}: a gathering needs the event to have a place`)
      if (s.kind === 'plague' && !e.place && !e.roams) out.push(`${at}: a plague needs the event to have a place, or to roam`)
      if (s.kind === 'plague' && (s.first < 1 || s.first > 50)) out.push(`${at}: between 1 and 50 fall ill first`)
      if (s.kind === 'reward' && !itm.has(s.item)) out.push(`${at}: no item called “${s.item}”`)
      if (s.kind === 'weather' && s.name && !wea.has(s.name)) out.push(`${at}: no weather called “${s.name}”`)
      if ((s.kind === 'spawn' || s.kind === 'clear') && !arch.has(s.archetype)) out.push(`${at}: no bot type called “${s.archetype}”`)
      if (s.kind === 'hit' && !attr.has(s.attr)) out.push(`${at}: no attribute called “${s.attr}”`)
      if (s.kind === 'drop' && !itm.has(s.item)) out.push(`${at}: no item called “${s.item}”`)
      if ((s.kind === 'spawn' || s.kind === 'hit' || s.kind === 'drop') && s.place && !place.has(s.place)) out.push(`${at}: no place called “${s.place}”`)
      if (s.kind === 'zone' && !(g.zones ?? []).some((z) => z.name === s.zone)) out.push(`${at}: no weather zone called “${s.zone}”`)
    })
  }
  const zn = new Set<string>()
  for (const z of g.zones ?? []) {
    if (zn.has(z.name)) out.push(`two weather zones are called “${z.name}”`)
    zn.add(z.name)
    if (!place.has(z.place)) out.push(`zone ${z.name}: no place called “${z.place}”`)
    if (!wea.has(z.weather)) out.push(`zone ${z.name}: no weather called “${z.weather}”`)
  }
  return [...out, ...questProblems(g), ...classProblems(g)]
}
