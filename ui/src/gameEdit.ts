import type { Act, ActionFile, Archetype, AttrFile, Cond, GameFile, Place, Route, Rule } from './api'

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
    home: null,
    rules: [],
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
      rules: a.rules.map((r) =>
        r.when.kind === 'attr_below' || r.when.kind === 'attr_above' ? { ...r, when: { ...r.when, attr: swap(r.when.attr) } } : r,
      ),
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
      rules: a.rules.filter((r) => !((r.when.kind === 'attr_below' || r.when.kind === 'attr_above') && r.when.attr === gone)),
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

// ------------------------------------------------------------ rules, places, routes

export const COND_KINDS: readonly { kind: Cond['kind']; label: string }[] = [
  { kind: 'always', label: 'always' },
  { kind: 'sees', label: 'sees someone' },
  { kind: 'attr_below', label: 'attribute is below' },
  { kind: 'attr_above', label: 'attribute is above' },
  { kind: 'in_place', label: 'is inside a place' },
  { kind: 'chance', label: 'by chance' },
]
export const ACT_KINDS: readonly { kind: Act['kind']; label: string }[] = [
  { kind: 'wander', label: 'wander' },
  { kind: 'idle', label: 'stand still' },
  { kind: 'chase', label: 'chase' },
  { kind: 'flee', label: 'flee' },
  { kind: 'attack', label: 'attack' },
  { kind: 'patrol', label: 'patrol a route' },
  { kind: 'goto', label: 'go to a place' },
  { kind: 'say', label: 'say' },
]

export function defaultCond(kind: Cond['kind'], g: GameFile): Cond {
  switch (kind) {
    case 'always':
      return { kind }
    case 'sees':
      return { kind, who: 'player', within: 30 }
    case 'attr_below':
    case 'attr_above': {
      const a = g.attrs[0]
      return { kind, attr: a?.name ?? '', value: a ? Math.round((a.min + a.max) / (kind === 'attr_below' ? 3 : 1.5)) : 0 }
    }
    case 'in_place':
      return { kind, place: g.places[0]?.name ?? '' }
    case 'chance':
      return { kind, pct: 10 }
  }
}

export function defaultAct(kind: Act['kind'], g: GameFile): Act {
  switch (kind) {
    case 'patrol':
      return { kind, route: g.routes[0]?.name ?? '' }
    case 'goto':
      return { kind, place: g.places[0]?.name ?? '' }
    case 'say':
      return { kind, text: 'Hello.', shout: false }
    default:
      return { kind }
  }
}

/** One plain sentence for a rule, used as its accessible name and in summaries. */
export function describeRule(r: Rule): string {
  const w = r.when
  const when =
    w.kind === 'always'
      ? 'Always'
      : w.kind === 'sees'
        ? `When it sees ${w.who === 'any' ? 'anyone' : w.who === 'bot' ? 'another bot' : 'a player'} within ${w.within}`
        : w.kind === 'attr_below'
          ? `When ${w.attr} is below ${w.value}`
          : w.kind === 'attr_above'
            ? `When ${w.attr} is above ${w.value}`
            : w.kind === 'in_place'
              ? `When inside ${w.place}`
              : `${w.pct}% of the time`
  const t = r.then
  const then =
    t.kind === 'patrol'
      ? `patrol ${t.route}`
      : t.kind === 'goto'
        ? `go to ${t.place}`
        : t.kind === 'say'
          ? `${t.shout ? 'shout' : 'say'} "${t.text}"`
          : ACT_KINDS.find((a) => a.kind === t.kind)?.label ?? t.kind
  return `${when}: ${then}`
}

function patchArch(g: GameFile, i: number, f: (a: Archetype) => Archetype): GameFile {
  return { ...g, archetypes: g.archetypes.map((a, k) => (k === i ? f(a) : a)) }
}

export const MAX_RULES = 16

export function addRule(g: GameFile, arch: number): GameFile {
  const a = g.archetypes[arch]
  if (!a || a.rules.length >= MAX_RULES) return g
  const rule: Rule = { when: defaultCond('always', g), then: defaultAct('wander', g) }
  return patchArch(g, arch, (x) => ({ ...x, rules: [...x.rules, rule] }))
}

export function updateRule(g: GameFile, arch: number, index: number, rule: Rule): GameFile {
  return patchArch(g, arch, (x) => ({ ...x, rules: x.rules.map((r, k) => (k === index ? rule : r)) }))
}

export function removeRule(g: GameFile, arch: number, index: number): GameFile {
  return patchArch(g, arch, (x) => ({ ...x, rules: x.rules.filter((_, k) => k !== index) }))
}

/** Move a rule so it ends up at `to` (after the removal), clamped to the list. */
export function moveRule(g: GameFile, arch: number, from: number, to: number): GameFile {
  return patchArch(g, arch, (x) => {
    if (from < 0 || from >= x.rules.length) return x
    const t = Math.max(0, Math.min(x.rules.length - 1, to))
    if (t === from) return x
    const rules = [...x.rules]
    const [r] = rules.splice(from, 1)
    rules.splice(t, 0, r as Rule)
    return { ...x, rules }
  })
}

/** Reorder rules to match a list of their current indices (what a drag gesture produces). */
export function reorderRules(g: GameFile, arch: number, order: number[]): GameFile {
  return patchArch(g, arch, (x) => {
    if (order.length !== x.rules.length || new Set(order).size !== order.length) return x
    return { ...x, rules: order.map((i) => x.rules[i] as Rule) }
  })
}

export function newPlace(g: GameFile, x: number, y: number): Place {
  return { name: uniqueName('place', g.places.map((p) => p.name)), x: Math.round(x), y: Math.round(y), r: 12 }
}

export function newRoute(g: GameFile, points: [number, number][]): Route {
  return {
    name: uniqueName('route', g.routes.map((p) => p.name)),
    points: points.map(([x, y]) => [Math.round(x), Math.round(y)]),
    loop: false,
  }
}

const refsPlace = (c: Cond | Act, name: string) =>
  (c.kind === 'in_place' || c.kind === 'goto') && c.place === name
const refsRoute = (a: Act, name: string) => a.kind === 'patrol' && a.route === name

export function renamePlace(g: GameFile, index: number, name: string): GameFile {
  const old = g.places[index]?.name
  if (old === undefined || old === name) return g
  const swapC = (c: Cond): Cond => (c.kind === 'in_place' && c.place === old ? { ...c, place: name } : c)
  const swapA = (a: Act): Act => (a.kind === 'goto' && a.place === old ? { ...a, place: name } : a)
  return {
    ...g,
    places: g.places.map((p, i) => (i === index ? { ...p, name } : p)),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      home: a.home === old ? name : a.home,
      rules: a.rules.map((r) => ({ when: swapC(r.when), then: swapA(r.then) })),
    })),
  }
}

/** Remove a place, and the rules and homes that pointed at it. */
export function removePlace(g: GameFile, index: number): GameFile {
  const gone = g.places[index]?.name
  if (gone === undefined) return g
  return {
    ...g,
    places: g.places.filter((_, i) => i !== index),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      home: a.home === gone ? null : a.home,
      rules: a.rules.filter((r) => !refsPlace(r.when, gone) && !refsPlace(r.then, gone)),
    })),
  }
}

export function renameRoute(g: GameFile, index: number, name: string): GameFile {
  const old = g.routes[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    routes: g.routes.map((r, i) => (i === index ? { ...r, name } : r)),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      rules: a.rules.map((r) => (r.then.kind === 'patrol' && r.then.route === old ? { ...r, then: { ...r.then, route: name } } : r)),
    })),
  }
}

export function removeRoute(g: GameFile, index: number): GameFile {
  const gone = g.routes[index]?.name
  if (gone === undefined) return g
  return {
    ...g,
    routes: g.routes.filter((_, i) => i !== index),
    archetypes: g.archetypes.map((a) => ({ ...a, rules: a.rules.filter((r) => !refsRoute(r.then, gone)) })),
  }
}

/** Keep a number inside a range. */
export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v))
}
