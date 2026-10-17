// Editing weather and events. They refer to attributes, bot types, places and each other by name, so renaming or
// deleting any of those has to rewrite them, the same way the bot rules are kept honest in gameEdit.ts.

import type { Archetype, EventDef, GameFile, ItemDef, Step, WeatherDef } from './api'
import { uniqueName } from './gameEdit'

export type RefKind = 'attr' | 'arch' | 'place' | 'weather' | 'item'

/** Rewrite every weather and event step that mentions `from`. With `to` null the references are removed: a drain,
 * a step, and an event left with no steps. (A deleted place drops its steps rather than widening them to "everywhere".) */
export function rewriteRefs(g: GameFile, kind: RefKind, from: string, to: string | null): GameFile {
  if (!g.weather && !g.events && kind !== 'item') return g
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
    }
  }
  const events = (g.events ?? [])
    .map((e) => ({ ...e, steps: e.steps.flatMap((s) => fixStep(s) ?? []) }))
    .filter((e) => e.steps.length > 0)
  // Loot tables name items too.
  const archetypes: Archetype[] =
    kind === 'item'
      ? g.archetypes.map((a) => (a.drops ? { ...a, drops: a.drops.flatMap((d) => (d.item !== from ? [d] : to === null ? [] : [{ ...d, item: to }])) } : a))
      : g.archetypes
  return { ...g, archetypes, weather, events }
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
export const MAX_STEPS = 12

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
]

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
  }
}

export function describeStep(s: Step): string {
  switch (s.kind) {
    case 'announce':
      return `say “${s.text}”`
    case 'weather':
      return s.name ? `weather ${s.name} for ${s.duration_secs}s` : 'clear the weather'
    case 'spawn':
      return `${s.count} ${s.archetype}${s.place ? ` at ${s.place}` : ''}`
    case 'hit':
      return `${s.attr} ${s.delta > 0 ? '+' : ''}${s.delta}${s.place ? ` at ${s.place}` : ' everywhere'}`
    case 'clear':
      return `remove every ${s.archetype}`
    case 'drop':
      return `${s.count} ${s.item} on the ground${s.place ? ` at ${s.place}` : ''}`
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
  for (const e of g.events ?? []) {
    e.steps.forEach((s, i) => {
      const at = `${e.name} step ${i + 1}`
      if (s.kind === 'weather' && s.name && !wea.has(s.name)) out.push(`${at}: no weather called “${s.name}”`)
      if ((s.kind === 'spawn' || s.kind === 'clear') && !arch.has(s.archetype)) out.push(`${at}: no bot type called “${s.archetype}”`)
      if (s.kind === 'hit' && !attr.has(s.attr)) out.push(`${at}: no attribute called “${s.attr}”`)
      if (s.kind === 'drop' && !itm.has(s.item)) out.push(`${at}: no item called “${s.item}”`)
      if ((s.kind === 'spawn' || s.kind === 'hit' || s.kind === 'drop') && s.place && !place.has(s.place)) out.push(`${at}: no place called “${s.place}”`)
    })
  }
  return out
}
