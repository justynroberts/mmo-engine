import type { ActionFile, Archetype, AttrFile, GameFile } from './api'

/** Everything that changes the wire schema (and so the handshake hash). */
export function wireOf(g: GameFile): string {
  return JSON.stringify({
    s: [g.move_speed, g.entity_radius, g.interest_radius, g.max_visible],
    a: g.attrs,
    c: g.actions,
  })
}
/** True when applying `draft` over `live` would restart the world and drop clients. */
export function needsReset(live: GameFile, draft: GameFile): boolean {
  return wireOf(live) !== wireOf(draft)
}

export function uniqueName(base: string, taken: string[]): string {
  if (!taken.includes(base)) return base
  for (let i = 2; ; i++) if (!taken.includes(`${base}_${i}`)) return `${base}_${i}`
}

export function newAttr(g: GameFile): AttrFile {
  return {
    name: uniqueName('attr', g.attrs.map((a) => a.name)),
    min: 0,
    max: 100,
    default: 100,
    bits: 8,
    replicated: false,
    regen_per_sec: 0,
    on_zero: 'none',
  }
}

export function newAction(g: GameFile): ActionFile {
  const used = new Set(g.actions.map((a) => a.id))
  let id = 1
  while (used.has(id)) id++
  return {
    id,
    name: uniqueName('action', g.actions.map((a) => a.name)),
    cooldown_ticks: 10,
    range: 8,
    requires: null,
    effects: [],
  }
}

export function newArchetype(g: GameFile): Archetype {
  return {
    name: uniqueName('bot', g.archetypes.map((a) => a.name)),
    color: '#4aa3a0',
    count: 0,
    behaviour: 'wander',
    speed_mult: 1,
    sight: 40,
    attack: null,
    attrs: {},
  }
}

/** Rename an attribute and every reference to it, so a rename never breaks the file. */
export function renameAttr(g: GameFile, index: number, name: string): GameFile {
  const old = g.attrs[index]?.name
  if (old === undefined || old === name) return g
  const swap = (n: string) => (n === old ? name : n)
  return {
    ...g,
    attrs: g.attrs.map((a, i) => (i === index ? { ...a, name } : a)),
    actions: g.actions.map((a) => ({
      ...a,
      requires: a.requires ? { ...a.requires, attr: swap(a.requires.attr) } : null,
      effects: a.effects.map((e) => ({ ...e, attr: swap(e.attr) })),
    })),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      attrs: Object.fromEntries(Object.entries(a.attrs).map(([k, v]) => [swap(k), v])),
    })),
  }
}

/** Remove an attribute and anything that referred to it. */
export function removeAttr(g: GameFile, index: number): GameFile {
  const gone = g.attrs[index]?.name
  if (gone === undefined) return g
  return {
    ...g,
    attrs: g.attrs.filter((_, i) => i !== index),
    actions: g.actions.map((a) => ({
      ...a,
      requires: a.requires?.attr === gone ? null : a.requires,
      effects: a.effects.filter((e) => e.attr !== gone),
    })),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      attrs: Object.fromEntries(Object.entries(a.attrs).filter(([k]) => k !== gone)),
    })),
  }
}

/** Rename an action and any bot that attacks with it. */
export function renameAction(g: GameFile, index: number, name: string): GameFile {
  const old = g.actions[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    actions: g.actions.map((a, i) => (i === index ? { ...a, name } : a)),
    archetypes: g.archetypes.map((a) => (a.attack === old ? { ...a, attack: name } : a)),
  }
}

export function removeAction(g: GameFile, index: number): GameFile {
  const gone = g.actions[index]?.name
  return {
    ...g,
    actions: g.actions.filter((_, i) => i !== index),
    archetypes: g.archetypes.map((a) => (a.attack === gone ? { ...a, attack: null } : a)),
  }
}

/** Parse a number the user typed; null while it is not (yet) a finite number. */
export function parseNum(s: string): number | null {
  if (s.trim() === '' || s.trim() === '-' || s.trim() === '.') return null
  const n = Number(s)
  return Number.isFinite(n) ? n : null
}
