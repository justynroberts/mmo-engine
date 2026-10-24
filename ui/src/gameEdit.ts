import type { Act, ActionFile, Archetype, AttrFile, ClassFile, Cond, EffectFile, GameFile, Place, QuestFile, QuestStep, Route, Rule } from './api'
import { rewriteRefs, type RefKind } from './directorEdit'

/** Everything that changes the wire schema (and so the handshake hash). Quests, levels, items and the rest are not
 *  in it: they apply live. */
export function wireOf(g: GameFile): string {
  return JSON.stringify({
    s: [g.move_speed, g.road_speed ?? 1.5, g.entity_radius, g.interest_radius, g.max_visible],
    a: g.attrs,
    // Only what the content hash covers: spell fields (aim, area, hits, hold, fx, bolt, school, effect secs) apply live.
    c: g.actions.map((a) => [a.id, a.name, a.cooldown_ticks, a.range, a.requires, a.effects.map((e) => [e.to, e.attr, e.delta])]),
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
  return rewriteRefs({
    ...g,
    attrs: g.attrs.map((a, i) => (i === index ? { ...a, name } : a)),
    ...(g.items ? { items: g.items.map((it) => ({ ...it, uses: it.uses.map((u) => ({ ...u, attr: swap(u.attr) })) })) } : {}),
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
  }, 'attr', old, name)
}

/** Remove an attribute and anything that referred to it. */
export function removeAttr(g: GameFile, index: number): GameFile {
  const gone = g.attrs[index]?.name
  if (gone === undefined) return g
  return rewriteRefs({
    ...g,
    attrs: g.attrs.filter((_, i) => i !== index),
    ...(g.items ? { items: g.items.map((it) => ({ ...it, uses: it.uses.filter((u) => u.attr !== gone) })) } : {}),
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
  }, 'attr', gone, null)
}

/** An action with its spell fields at their defaults left out, the way the server writes it, so the game file stays
 *  clean: aim 'target', area 0, hits 'any', no hold, no fx, bolt false, empty school and effect secs 0 are absent. */
export function cleanAction(a: ActionFile): ActionFile {
  const out: ActionFile = { ...a, effects: a.effects.map(cleanEffect) }
  if (!out.aim || out.aim === 'target') delete out.aim
  if (!out.area) delete out.area
  if (!out.hits || out.hits === 'any') delete out.hits
  if (!out.hold) delete out.hold
  if (!out.fx) delete out.fx
  if (!out.bolt) delete out.bolt
  if (!out.school) delete out.school
  if (!out.desc) delete out.desc
  if (!out.lift) delete out.lift
  if (!out.level || out.level <= 1) delete out.level
  return out
}

function cleanEffect(e: EffectFile): EffectFile {
  const out = { ...e }
  if (!out.secs) delete out.secs
  if (!out.lasts) delete out.lasts
  return out
}

/** Rename an action and any bot that attacks with it. */
export function renameAction(g: GameFile, index: number, name: string): GameFile {
  const old = g.actions[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    actions: g.actions.map((a, i) => (i === index ? { ...a, name } : a)),
    archetypes: g.archetypes.map((a) => (a.attack === old ? { ...a, attack: name } : a)),
    ...(g.classes ? { classes: g.classes.map((c) => (c.spells?.includes(old) ? { ...c, spells: c.spells.map((n) => (n === old ? name : n)) } : c)) } : {}),
  }
}

export function removeAction(g: GameFile, index: number): GameFile {
  const gone = g.actions[index]?.name
  return {
    ...g,
    actions: g.actions.filter((_, i) => i !== index),
    archetypes: g.archetypes.map((a) => (a.attack === gone ? { ...a, attack: null } : a)),
    // A class's spells drop it (a class left with none may then use every action, as an empty list means).
    ...(g.classes && gone !== undefined
      ? { classes: g.classes.map((c) => (c.spells?.includes(gone) ? cleanClass({ ...c, spells: c.spells.filter((n) => n !== gone) }) : c)) }
      : {}),
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
  { kind: 'sees_kind', label: 'sees a bot type' },
  { kind: 'during', label: 'at a time of day' },
  { kind: 'weather', label: 'in weather' },
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
  { kind: 'chat', label: 'pass a remark' },
  { kind: 'indoors', label: 'go home, indoors' },
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
    case 'sees_kind':
      return { kind, bot: g.archetypes[0]?.name ?? '', within: 30 }
    case 'during':
      return { kind, parts: ['dusk', 'night'] }
    case 'weather':
      return { kind, names: [] }
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
    case 'chat':
      return { kind, lines: ['Fine day.'] }
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
              : w.kind === 'sees_kind'
                ? `When it sees a ${w.bot} within ${w.within}`
                : w.kind === 'during'
                  ? `At ${w.parts.join(' or ')}`
                  : w.kind === 'weather'
                    ? w.names.length ? `In ${w.names.join(' or ')}` : 'In any weather'
                    : `${w.pct}% of the time`
  const t = r.then
  const then =
    t.kind === 'patrol'
      ? `patrol ${t.route}`
      : t.kind === 'goto'
        ? `go to ${t.place}`
        : t.kind === 'say'
          ? `${t.shout ? 'shout' : 'say'} "${t.text}"`
          : t.kind === 'chat'
            ? `remark "${t.lines[0] ?? ''}"${t.lines.length > 1 ? ` (or ${t.lines.length - 1} more)` : ''}`
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
  return rewriteRefs({
    ...g,
    places: g.places.map((p, i) => (i === index ? { ...p, name } : p)),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      home: a.home === old ? name : a.home,
      rules: a.rules.map((r) => ({ when: swapC(r.when), then: swapA(r.then) })),
    })),
  }, 'place', old, name)
}

/** Remove a place, and the rules and homes that pointed at it. */
export function removePlace(g: GameFile, index: number): GameFile {
  const gone = g.places[index]?.name
  if (gone === undefined) return g
  return rewriteRefs({
    ...g,
    places: g.places.filter((_, i) => i !== index),
    archetypes: g.archetypes.map((a) => ({
      ...a,
      home: a.home === gone ? null : a.home,
      // Going indoors means going home: with the home gone, so are those rules.
      rules: a.rules.filter((r) => !refsPlace(r.when, gone) && !refsPlace(r.then, gone) && !(a.home === gone && r.then.kind === 'indoors')),
    })),
  }, 'place', gone, null)
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
    const known = new Set(['name', 'color', 'count', 'behaviour', 'speed_mult', 'sight', 'attack', 'attrs', 'home', 'rules', 'drops', 'xp', 'level', 'shop'])
    const stray = Object.keys(r).find((k) => !known.has(k))
    if (stray) return { error: `${r.name}: “${stray}” is not a bot field. Check the spelling.` }
    const a: Archetype = { ...newArchetype(g), ...r, name: r.name.trim() }
    if (!['idle', 'wander', 'chase', 'flee'].includes(a.behaviour)) return { error: `${a.name}: behaviour must be idle, wander, chase or flee.` }
    if (a.attack && !g.actions.some((x) => x.name === a.attack)) return { error: `${a.name}: there is no action called “${a.attack}” in this game.` }
    if (a.home && !g.places.some((p) => p.name === a.home)) return { error: `${a.name}: there is no place called “${a.home}”.` }
    const badAttr = Object.keys(a.attrs).find((k) => !g.attrs.some((x) => x.name === k))
    if (badAttr) return { error: `${a.name}: there is no attribute called “${badAttr}”.` }
    const badItem = [...(a.drops ?? []), ...(a.shop ?? [])].find((d) => !(g.items ?? []).some((x) => x.name === d.item))
    if (badItem) return { error: `${a.name}: there is no item called “${badItem.item}”.` }
    out.push(a)
  }
  let archetypes = g.archetypes
  for (const a of out) {
    const at = archetypes.findIndex((x) => x.name === a.name)
    archetypes = at >= 0 ? archetypes.map((x, k) => (k === at ? a : x)) : [...archetypes, a]
  }
  return { game: { ...g, archetypes }, names: out.map((a) => a.name) }
}

// ---------------------------------------------------------------- quests

export const MAX_QUESTS = 64
export const MAX_QUEST_STEPS = 8
export const MAX_QUEST_REWARDS = 8

export const QUEST_STEP_KINDS: readonly { kind: QuestStep['kind']; label: string }[] = [
  { kind: 'kill', label: 'Beat bots' },
  { kind: 'visit', label: 'Go to a place' },
  { kind: 'collect', label: 'Bring items' },
  { kind: 'talk', label: 'Talk to a bot' },
]

/** A new step of a kind, naming the first thing of its sort the game has ('' where it has none). */
export function defaultQuestStep(kind: QuestStep['kind'], g: GameFile): QuestStep {
  switch (kind) {
    case 'kill':
      return { kind, archetype: g.archetypes[0]?.name ?? '', count: 5 }
    case 'visit':
      return { kind, place: g.places[0]?.name ?? '' }
    case 'collect':
      return { kind, item: g.items?.[0]?.name ?? '', count: 3 }
    case 'talk':
      return { kind, archetype: g.archetypes[0]?.name ?? '' }
  }
}

/** A new quest given by the first bot type, with one step; null when there is no bot type to give it. */
export function newQuest(g: GameFile): QuestFile | null {
  const giver = g.archetypes[0]?.name
  if (!giver) return null
  const name = uniqueName('quest', (g.quests ?? []).map((q) => q.name))
  const first: QuestStep = g.places[0] ? defaultQuestStep('visit', g) : defaultQuestStep('kill', g)
  return { name, title: 'A new quest', text: 'There is something you could do for me.', giver, steps: [first] }
}

/** A quest with its optional fields left out when empty or at their defaults, the way the server writes it. */
export function cleanQuest(q: QuestFile): QuestFile {
  const out: QuestFile = { ...q }
  if (!out.done_text) delete out.done_text
  if (!out.turn_in || out.turn_in === out.giver) delete out.turn_in
  if (!out.level || out.level <= 1) delete out.level
  if (!out.after) delete out.after
  if (!out.xp || out.xp <= 0) delete out.xp
  if (!out.reward?.length) delete out.reward
  if (!out.repeatable) delete out.repeatable
  return out
}

export function renameQuest(g: GameFile, index: number, name: string): GameFile {
  const old = g.quests?.[index]?.name
  if (old === undefined || old === name) return g
  return {
    ...g,
    quests: (g.quests ?? []).map((q, i) => {
      const n = i === index ? { ...q, name } : q
      return n.after === old ? { ...n, after: name } : n
    }),
  }
}

/** Remove a quest; the quests that came after it no longer wait for it. */
export function removeQuest(g: GameFile, index: number): GameFile {
  const gone = g.quests?.[index]?.name
  if (gone === undefined) return g
  return { ...g, quests: dropAfter((g.quests ?? []).filter((_, i) => i !== index), new Set([gone])) }
}

function dropAfter(quests: QuestFile[], gone: Set<string>): QuestFile[] {
  return quests.map((q) => {
    if (!q.after || !gone.has(q.after)) return q
    const { after: _a, ...rest } = q
    void _a
    return rest
  })
}

/** Rewrite quest references to a bot type, place or item that was renamed (`to`) or removed (`to` null). A quest
 *  whose giver goes is dropped; a step or reward whose target goes is dropped, and a quest left with no steps too;
 *  a turn-in bot type that goes falls back to the giver; quests that came after a dropped one stop waiting for it. */
export function rewriteQuestRefs(quests: QuestFile[], kind: RefKind, from: string, to: string | null): QuestFile[] {
  if (kind !== 'arch' && kind !== 'place' && kind !== 'item') return quests
  const swap = (n: string) => (n === from ? to : n)
  const fixStep = (s: QuestStep): QuestStep | null => {
    if ((s.kind === 'kill' || s.kind === 'talk') && kind === 'arch' && s.archetype === from) return to === null ? null : { ...s, archetype: to }
    if (s.kind === 'visit' && kind === 'place' && s.place === from) return to === null ? null : { ...s, place: to }
    if (s.kind === 'collect' && kind === 'item' && s.item === from) return to === null ? null : { ...s, item: to }
    return s
  }
  const kept: QuestFile[] = []
  const dropped = new Set<string>()
  for (const q of quests) {
    if (kind === 'arch' && q.giver === from && to === null) {
      dropped.add(q.name)
      continue
    }
    const out: QuestFile = { ...q, steps: q.steps.flatMap((s) => fixStep(s) ?? []) }
    if (out.steps.length === 0) {
      dropped.add(q.name)
      continue
    }
    if (kind === 'arch') {
      out.giver = swap(q.giver) ?? q.giver
      if (q.turn_in !== undefined) {
        const t = swap(q.turn_in)
        if (t === null) delete out.turn_in
        else out.turn_in = t
      }
    }
    if (kind === 'item' && q.reward) out.reward = q.reward.flatMap((r) => (r.item !== from ? [r] : to === null ? [] : [{ ...r, item: to }]))
    kept.push(cleanQuest(out))
  }
  return dropped.size ? dropAfter(kept, dropped) : kept
}

/** What stops the quests being valid for the server, in words (the server checks the same things). */
export function questProblems(g: GameFile): string[] {
  const out: string[] = []
  const quests = g.quests ?? []
  if (quests.length > MAX_QUESTS) out.push(`at most ${MAX_QUESTS} quests`)
  const arch = new Set(g.archetypes.map((a) => a.name))
  const place = new Set(g.places.map((p) => p.name))
  const itm = new Set((g.items ?? []).map((i) => i.name))
  const names = new Set<string>()
  const bytes = (s: string) => new TextEncoder().encode(s).length
  for (const q of quests) {
    const at = `quest ${q.name}`
    if (!q.name.trim() || bytes(q.name) > 48) out.push(`${at}: a name of 1 to 48 bytes`)
    if (names.has(q.name)) out.push(`two quests are called “${q.name}”`)
    names.add(q.name)
    if (!q.title.trim() || [...q.title].length > 64) out.push(`${at}: a title of 1 to 64 characters`)
    if ([...(q.text ?? '')].length > 600 || [...(q.done_text ?? '')].length > 600) out.push(`${at}: texts of at most 600 characters`)
    if (!arch.has(q.giver)) out.push(`${at}: no bot type called “${q.giver}” to give it`)
    if (q.turn_in && !arch.has(q.turn_in)) out.push(`${at}: no bot type called “${q.turn_in}” to hand it in to`)
    if (q.after && (q.after === q.name || !quests.some((o) => o.name === q.after))) out.push(`${at}: comes after “${q.after}”, which is not another quest`)
    if (q.steps.length === 0 || q.steps.length > MAX_QUEST_STEPS) out.push(`${at}: needs 1 to ${MAX_QUEST_STEPS} steps`)
    q.steps.forEach((s, i) => {
      const st = `${at} step ${i + 1}`
      if ((s.kind === 'kill' || s.kind === 'talk') && !arch.has(s.archetype)) out.push(`${st}: no bot type called “${s.archetype}”`)
      if (s.kind === 'visit' && !place.has(s.place)) out.push(`${st}: no place called “${s.place}”`)
      if (s.kind === 'collect' && !itm.has(s.item)) out.push(`${st}: no item called “${s.item}”`)
      if ((s.kind === 'kill' || s.kind === 'collect') && !(s.count >= 1)) out.push(`${st}: a count of at least 1`)
    })
    for (const r of q.reward ?? []) {
      if (!itm.has(r.item)) out.push(`${at}: rewards “${r.item}”, which is not an item`)
      if (!(r.count >= 1)) out.push(`${at}: reward counts of at least 1`)
    }
  }
  return out
}

// ---------------------------------------------------------------- classes

export const MAX_CLASSES = 15

/** A new class with nothing set but its name and title. */
export function newClass(g: GameFile): ClassFile {
  return { name: uniqueName('class', (g.classes ?? []).map((c) => c.name)), title: 'A new class' }
}

/** A class with its optional fields left out when empty or zero, the way the server writes it. */
export function cleanClass(c: ClassFile): ClassFile {
  const out: ClassFile = { ...c }
  if (!out.desc) delete out.desc
  if (!out.look) delete out.look
  if (!out.might) delete out.might
  if (!out.toughness) delete out.toughness
  if (!out.spells?.length) delete out.spells
  return out
}

/** Nothing refers to a class by name (players keep theirs by name on the server), so renaming is just that. */
export function renameClass(g: GameFile, index: number, name: string): GameFile {
  const old = g.classes?.[index]?.name
  if (old === undefined || old === name) return g
  return { ...g, classes: (g.classes ?? []).map((c, i) => (i === index ? { ...c, name } : c)) }
}

export function removeClass(g: GameFile, index: number): GameFile {
  if (!g.classes?.[index]) return g
  return { ...g, classes: g.classes.filter((_, i) => i !== index) }
}

/** What stops the classes being valid for the server, in words (the server checks the same things). */
export function classProblems(g: GameFile): string[] {
  const out: string[] = []
  const classes = g.classes ?? []
  if (classes.length > MAX_CLASSES) out.push(`at most ${MAX_CLASSES} classes`)
  const acts = new Set(g.actions.map((a) => a.name))
  const names = new Set<string>()
  const bytes = (s: string) => new TextEncoder().encode(s).length
  const share = (n: number | undefined) => n === undefined || (Number.isFinite(n) && n >= 0 && n <= 2)
  for (const c of classes) {
    const at = `class ${c.name}`
    if (!c.name.trim() || bytes(c.name) > 32) out.push(`${at}: a name of 1 to 32 bytes`)
    if (names.has(c.name)) out.push(`two classes are called “${c.name}”`)
    names.add(c.name)
    if (!c.title.trim() || [...c.title].length > 32) out.push(`${at}: a title of 1 to 32 characters`)
    if ([...(c.desc ?? '')].length > 300) out.push(`${at}: a description of at most 300 characters`)
    if (!share(c.might) || !share(c.toughness)) out.push(`${at}: might and toughness from 0 to 2`)
    for (const n of c.spells ?? []) if (!acts.has(n)) out.push(`${at}: “${n}” is not an action`)
  }
  return out
}
