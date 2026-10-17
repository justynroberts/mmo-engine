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

/** Walls smaller than this (in world units) on either side are treated as an accidental click. */
export const MIN_WALL = 3
export const MAX_WALLS = 2000

/** A dragged rectangle as a wall: corners sorted, rounded, kept inside the world. `null` if it is too small. */
export function normWall(ax: number, ay: number, bx: number, by: number, size: number): [number, number, number, number] | null {
  const c = (v: number) => Math.min(size, Math.max(0, Math.round(v)))
  const [x0, x1] = [c(Math.min(ax, bx)), c(Math.max(ax, bx))]
  const [y0, y1] = [c(Math.min(ay, by)), c(Math.max(ay, by))]
  return x1 - x0 >= MIN_WALL && y1 - y0 >= MIN_WALL ? [x0, y0, x1, y1] : null
}

/** Turn hand editing on, starting from the walls the world has now. */
export function takeOverWalls(g: GameFile, current: [number, number, number, number][]): GameFile {
  return { ...g, walls: current.map((w) => [...w] as [number, number, number, number]) }
}

/** Back to the walls generated from the seed. */
export function generatedWalls(g: GameFile): GameFile {
  const { walls: _drop, ...rest } = g
  void _drop
  return rest
}

export function addWall(g: GameFile, w: [number, number, number, number]): GameFile {
  const walls = g.walls ?? []
  return walls.length >= MAX_WALLS ? g : { ...g, walls: [...walls, w] }
}

export function removeWall(g: GameFile, i: number): GameFile {
  return { ...g, walls: (g.walls ?? []).filter((_, k) => k !== i) }
}

/** Set one edge value of a wall, keeping it a valid rectangle inside the world. */
export function setWallEdge(g: GameFile, i: number, edge: 0 | 1 | 2 | 3, v: number, size: number): GameFile {
  const w = [...(g.walls?.[i] ?? [0, 0, 0, 0])] as [number, number, number, number]
  const lo = edge < 2 ? 0 : w[edge - 2]! + 1
  const hi = edge < 2 ? w[edge + 2]! - 1 : size
  w[edge] = Math.min(hi, Math.max(lo, Math.round(v)))
  return { ...g, walls: (g.walls ?? []).map((x, k) => (k === i ? w : x)) }
}

// ---------------------------------------------------------------- starting a bot type quickly

export interface BotTemplate {
  id: string
  label: string
  make: (g: GameFile) => Archetype
}

function base(g: GameFile, name: string, color: string, p: Partial<Archetype>): Archetype {
  return { ...newArchetype(g), name: uniqueName(name, g.archetypes.map((a) => a.name)), color, count: 3, ...p }
}

/** Ready-made starting points. Each only names things that exist in the current game (the first action, the
 * first route), so a template never produces a file the server would refuse. */
export const BOT_TEMPLATES: readonly BotTemplate[] = [
  { id: 'wanderer', label: 'Wanderer: drifts about', make: (g) => base(g, 'wanderer', '#4aa3a0', { behaviour: 'wander', speed_mult: 0.8 }) },
  {
    id: 'guard',
    label: 'Guard: stands still, fights what comes near',
    make: (g) =>
      base(g, 'guard', '#ff3df0', {
        behaviour: 'idle',
        sight: 50,
        attack: g.actions[0]?.name ?? null,
        rules: g.actions[0] ? [{ when: { kind: 'sees', who: 'player', within: 30 }, then: { kind: 'attack' } }] : [],
      }),
  },
  {
    id: 'hunter',
    label: 'Hunter: chases and attacks',
    make: (g) => base(g, 'hunter', '#e0457b', { behaviour: 'chase', speed_mult: 1.1, sight: 70, attack: g.actions[0]?.name ?? null }),
  },
  {
    id: 'coward',
    label: 'Coward: runs from anything it sees',
    make: (g) =>
      base(g, 'coward', '#e8c547', {
        behaviour: 'wander',
        speed_mult: 1.2,
        sight: 50,
        rules: [{ when: { kind: 'sees', who: 'any', within: 35 }, then: { kind: 'flee' } }],
      }),
  },
  {
    id: 'patroller',
    label: 'Patroller: walks the first route',
    make: (g) =>
      base(g, 'patroller', '#7ac2ff', {
        behaviour: 'wander',
        rules: g.routes[0] ? [{ when: { kind: 'always' }, then: { kind: 'patrol', route: g.routes[0].name } }] : [],
      }),
  },
  {
    id: 'greeter',
    label: 'Greeter: says hello to players',
    make: (g) =>
      base(g, 'greeter', '#b5e853', {
        behaviour: 'wander',
        speed_mult: 0.5,
        rules: [{ when: { kind: 'sees', who: 'player', within: 25 }, then: { kind: 'say', text: 'Hello there.', shout: false } }],
      }),
  },
]

export function addFromTemplate(g: GameFile, id: string): GameFile {
  const t = BOT_TEMPLATES.find((x) => x.id === id)
  return t ? { ...g, archetypes: [...g.archetypes, t.make(g)] } : g
}

/** The JSON for one bot type, exactly what the server's bots folder accepts. */
export function exportArchetype(a: Archetype): string {
  return JSON.stringify(a, null, 2)
}

/** Add bot types from pasted JSON (one object or an array). A bot with the name of an existing one replaces it,
 * which matches how files in the server's bots folder behave. Nothing is changed if anything is wrong. */
export function importArchetypes(g: GameFile, text: string): { game: GameFile; names: string[] } | { error: string } {
  let v: unknown
  try {
    v = JSON.parse(text)
  } catch (e) {
    return { error: `That is not valid JSON: ${(e as Error).message}` }
  }
  const items = Array.isArray(v) ? v : [v]
  if (items.length === 0) return { error: 'There is nothing in that to import.' }
  const out: Archetype[] = []
  for (const raw of items) {
    if (typeof raw !== 'object' || raw === null || typeof (raw as { name?: unknown }).name !== 'string' || !(raw as { name: string }).name.trim()) {
      return { error: 'Each bot type needs a name.' }
    }
    const r = raw as Partial<Archetype> & { name: string }
    const known = new Set(['name', 'color', 'count', 'behaviour', 'speed_mult', 'sight', 'attack', 'attrs', 'home', 'rules'])
    const stray = Object.keys(r).find((k) => !known.has(k))
    if (stray) return { error: `${r.name}: “${stray}” is not a bot field. Check the spelling.` }
    const a: Archetype = { ...newArchetype(g), ...r, name: r.name.trim() }
    if (!['idle', 'wander', 'chase', 'flee'].includes(a.behaviour)) return { error: `${a.name}: behaviour must be idle, wander, chase or flee.` }
    if (a.attack && !g.actions.some((x) => x.name === a.attack)) return { error: `${a.name}: there is no action called “${a.attack}” in this game.` }
    if (a.home && !g.places.some((p) => p.name === a.home)) return { error: `${a.name}: there is no place called “${a.home}”.` }
    const badAttr = Object.keys(a.attrs).find((k) => !g.attrs.some((x) => x.name === k))
    if (badAttr) return { error: `${a.name}: there is no attribute called “${badAttr}”.` }
    out.push(a)
  }
  let archetypes = g.archetypes
  for (const a of out) {
    const at = archetypes.findIndex((x) => x.name === a.name)
    archetypes = at >= 0 ? archetypes.map((x, k) => (k === at ? a : x)) : [...archetypes, a]
  }
  return { game: { ...g, archetypes }, names: out.map((a) => a.name) }
}
