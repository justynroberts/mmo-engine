// What the MCP server can do: everything the console does, as tools an AI can call. Every tool goes through the
// server's admin API (/api/*), and game file edits use the console's own editing functions (gameEdit.ts,
// directorEdit.ts), so a rename or a removal here rewrites the references to it exactly as the console does.

import { z } from 'zod'
import type { Archetype, EntityRow, GameFile, Info, PutResult, State } from '../src/api'
import { KIND_BOT, KIND_DRIVEN, KIND_GHOST, KIND_PLAYER } from '../src/api'
import { DEFAULT_DAYLIGHT, describe, moment, phase } from '../src/daylight'
import type { Daylight } from '../src/daylight'
import {
  addEvent,
  addWeather,
  directorProblems,
  EVENT_PRESETS,
  newItem,
  removeArch,
  removeEvent,
  removeItem,
  removeWeather,
  removeZone,
  renameArch,
  renameEvent,
  renameItem,
  renameWeather,
  renameZone,
  WEATHER_PRESETS,
} from '../src/directorEdit'
import {
  addFromTemplate,
  BOT_TEMPLATES,
  generatedWalls,
  needsReset,
  newAction,
  newArchetype,
  newAttr,
  removeAction,
  removeAttr,
  removePlace,
  removeRoute,
  renameAction,
  renameAttr,
  renamePlace,
  renameRoute,
} from '../src/gameEdit'
import { defaultRecipe } from '../src/world'
import apiTypes from '../src/api.ts?raw'

/** How a tool reaches the server: one admin API request, its status and parsed body. */
export interface Ctx {
  call(path: string, method?: string, body?: unknown): Promise<{ status: number; body: any }>
  /** Point at another server (another zone's admin port). */
  setBase(url: string): void
  base(): string
}

export interface ToolDef {
  name: string
  description: string
  input: z.ZodRawShape
  /** Changes nothing. */
  readOnly?: boolean
  /** Can take away what someone made (blocks, bots, the land). */
  destructive?: boolean
  run(args: any, ctx: Ctx): Promise<unknown>
}

/** A refusal the AI can act on: it becomes the tool's error text. */
export class Refused extends Error {}

const refuse = (msg: string): never => {
  throw new Refused(msg)
}

async function ok<T = any>(ctx: Ctx, path: string, method = 'GET', body?: unknown): Promise<T> {
  const r = await ctx.call(path, method, body)
  if (r.status >= 400 || r.body?.ok === false) refuse(r.body?.error ?? `${method} ${path}: HTTP ${r.status}`)
  return r.body as T
}

// ---------------------------------------------------------------- the game file

type Section = 'places' | 'routes' | 'archetypes' | 'items' | 'weather' | 'events' | 'zones' | 'attrs' | 'actions'

interface SectionOps {
  list(g: GameFile): { name: string }[]
  put(g: GameFile, list: any[]): GameFile
  /** A new entry with every field filled in, before the caller's fields go over it. */
  fresh(g: GameFile): any
  rename(g: GameFile, i: number, name: string): GameFile
  remove(g: GameFile, i: number): GameFile
}

const SECTIONS: Record<Section, SectionOps> = {
  places: { list: (g) => g.places, put: (g, l) => ({ ...g, places: l }), fresh: () => ({ x: 0, y: 0, r: 12 }), rename: renamePlace, remove: removePlace },
  routes: { list: (g) => g.routes, put: (g, l) => ({ ...g, routes: l }), fresh: () => ({ points: [], loop: false }), rename: renameRoute, remove: removeRoute },
  archetypes: { list: (g) => g.archetypes, put: (g, l) => ({ ...g, archetypes: l }), fresh: newArchetype, rename: renameArch, remove: removeArch },
  items: { list: (g) => g.items ?? [], put: (g, l) => ({ ...g, items: l }), fresh: newItem, rename: renameItem, remove: removeItem },
  weather: {
    list: (g) => g.weather ?? [],
    put: (g, l) => ({ ...g, weather: l }),
    fresh: () => ({ color: '#8899aa', intensity: 0.5, sight_mult: 1, drain: [] }),
    rename: renameWeather,
    remove: removeWeather,
  },
  events: { list: (g) => g.events ?? [], put: (g, l) => ({ ...g, events: l }), fresh: () => ({ every_secs: 0, chance_pct: 100, steps: [] }), rename: renameEvent, remove: removeEvent },
  zones: { list: (g) => g.zones ?? [], put: (g, l) => ({ ...g, zones: l }), fresh: () => ({ active: true, height: 30 }), rename: renameZone, remove: removeZone },
  attrs: { list: (g) => g.attrs, put: (g, l) => ({ ...g, attrs: l }), fresh: newAttr, rename: renameAttr, remove: removeAttr },
  actions: { list: (g) => g.actions, put: (g, l) => ({ ...g, actions: l }), fresh: newAction, rename: renameAction, remove: removeAction },
}
const SECTION_NAMES = Object.keys(SECTIONS) as [Section, ...Section[]]

const RESTART_NOTE =
  'This changes attributes, actions or movement settings, which restarts the world and disconnects every player. Call again with allow_restart: true if that is what you want.'

/** Read the live game file, change it, and apply it: what the console's Apply does. */
async function edit(ctx: Ctx, change: (g: GameFile) => GameFile, allowRestart = false): Promise<{ applied: PutResult; game: GameFile }> {
  const live = await ok<GameFile>(ctx, '/api/game')
  const next = change(live)
  if (next === live) refuse('Nothing to change.')
  if (needsReset(live, next) && !allowRestart) refuse(RESTART_NOTE)
  const problems = directorProblems(next)
  if (problems.length) refuse(`The game file would not be valid:\n- ${problems.join('\n- ')}`)
  const r = await ctx.call('/api/game', 'PUT', next)
  const applied = r.body as PutResult
  if (r.status >= 400 || !applied?.ok) refuse(`The server refused the change: ${applied?.error ?? `HTTP ${r.status}`}`)
  return { applied, game: next }
}

const indexOf = (g: GameFile, s: Section, name: string): number => {
  const i = SECTIONS[s].list(g).findIndex((e) => e.name === name)
  if (i < 0) refuse(`There is no ${s} entry called "${name}". It has: ${SECTIONS[s].list(g).map((e) => e.name).join(', ') || 'none'}.`)
  return i
}

/** What came of an apply, without the noise. */
const outcome = (a: PutResult) => ({ applied: true, restarted: !!a.reset, ...(a.reset ? { dropped_clients: a.dropped_clients } : {}), saved: a.saved ?? null })

// ---------------------------------------------------------------- live state

const KIND_NAME: Record<number, string> = { [KIND_BOT]: 'bot', [KIND_PLAYER]: 'player', [KIND_DRIVEN]: 'driven', [KIND_GHOST]: 'ghost' }

function entity(row: EntityRow, s: State, game: GameFile | null) {
  const [id, x, y, arch, kind, ...vals] = row as [number, number, number, number, number, ...number[]]
  return {
    id,
    kind: KIND_NAME[kind] ?? String(kind),
    type: arch >= 0 ? (game?.archetypes[arch]?.name ?? arch) : null,
    ...(s.names?.[String(id)] ? { name: s.names[String(id)] } : {}),
    x: Math.round(x * 10) / 10,
    y: Math.round(y * 10) / 10,
    attrs: Object.fromEntries(s.attrs.map((a, i) => [a, vals[i]])),
  }
}

const dist = (ax: number, ay: number, bx: number, by: number) => Math.hypot(ax - bx, ay - by)

async function placeArea(ctx: Ctx, name: string): Promise<{ x: number; y: number; r: number }> {
  const g = await ok<GameFile>(ctx, '/api/game')
  const p = g.places.find((p) => p.name === name)
  if (!p) refuse(`There is no place called "${name}". Places: ${g.places.map((p) => p.name).join(', ') || 'none'}.`)
  return { x: p!.x, y: p!.y, r: p!.r }
}

// ---------------------------------------------------------------- blocks

/** Block size in world units and the layers in a column (proto::world::BLOCK, COLUMN_LAYERS). */
export const BLOCK = 2
export const LAYERS = 16
/** The materials, by number (godot/scripts/theme.gd BLOCK_NAMES; proto::game::BLOCK_KINDS). */
export const MATERIALS = ['stone', 'timber', 'red brick', 'plaster', 'brick', 'rough brick', 'roof tiles', 'iron', 'glass', 'slate', 'mossy stone', 'sandstone', 'lamp', 'fire']
const MAX_CELLS = 20000

export function materialChar(m: string | number): string {
  const k = typeof m === 'number' ? m : MATERIALS.indexOf(m.trim().toLowerCase())
  if (!Number.isInteger(k) || k < 0 || k >= MATERIALS.length) refuse(`Unknown material "${m}". Materials: ${MATERIALS.join(', ')} (or 0 to ${MATERIALS.length - 1}).`)
  return k.toString(36)
}

/** The block cells a shape covers, given in world units. */
export function shapeCells(shape: 'fill' | 'walls' | 'line', x0: number, y0: number, x1: number, y1: number): [number, number][] {
  const c = (v: number) => Math.floor(v / BLOCK)
  let [ax, ay, bx, by] = [c(x0), c(y0), c(x1), c(y1)]
  if (shape === 'line') {
    const out: [number, number][] = []
    const n = Math.max(Math.abs(bx - ax), Math.abs(by - ay))
    if (n > MAX_CELLS) refuse(`That line is ${n} blocks long; at most ${MAX_CELLS}.`)
    for (let i = 0; i <= n; i++) {
      const t = n === 0 ? 0 : i / n
      const p: [number, number] = [Math.round(ax + (bx - ax) * t), Math.round(ay + (by - ay) * t)]
      if (!out.some((q) => q[0] === p[0] && q[1] === p[1])) out.push(p)
    }
    return out
  }
  ;[ax, bx] = [Math.min(ax, bx), Math.max(ax, bx)]
  ;[ay, by] = [Math.min(ay, by), Math.max(ay, by)]
  const area = (bx - ax + 1) * (by - ay + 1)
  if (area > MAX_CELLS) refuse(`That covers ${area} block cells; at most ${MAX_CELLS} in one go.`)
  const out: [number, number][] = []
  for (let cy = ay; cy <= by; cy++)
    for (let cx = ax; cx <= bx; cx++) if (shape === 'fill' || cx === ax || cx === bx || cy === ay || cy === by) out.push([cx, cy])
  return out
}

/** Lay `ch` (or '.' to clear) over layers [bottom, bottom + height) of an existing column's text. */
export function overlay(existing: string, bottom: number, height: number, ch: string): string {
  const cells = existing.padEnd(LAYERS, '.').split('')
  for (let k = bottom; k < bottom + height && k < LAYERS; k++) cells[k] = ch
  return cells.join('').replace(/\.+$/, '')
}

// ---------------------------------------------------------------- the tools

const num = z.number()
const area = {
  place: z.string().optional().describe('Only inside this place.'),
  x: num.optional().describe('With y and r: only inside this circle (world units).'),
  y: num.optional(),
  r: num.optional(),
}
const allowRestart = z.boolean().optional().describe('Needed for changes to attributes, actions or movement settings: they restart the world and disconnect every player.')

export const TOOLS: ToolDef[] = [
  {
    name: 'use_server',
    description:
      'Choose which server to control by its admin URL (default http://127.0.0.1:7780, or MMO_ADMIN). With zone lines or strips each zone is its own server with its own admin port, so switch to work on another zone.',
    input: { admin: z.string().describe('For example http://127.0.0.1:7980') },
    async run({ admin }, ctx) {
      ctx.setBase(admin)
      const info = await ok<Info>(ctx, '/api/info')
      return { now: ctx.base(), world_size: info.world_size }
    },
  },
  {
    name: 'overview',
    description:
      'Start here. The server and world at a glance: world size, players and bots by type, server load, weather, time of day, running events, this zone, blocks built, and the names of every place, bot type, item, weather and event.',
    input: {},
    readOnly: true,
    async run(_, ctx) {
      const [info, s, g] = await Promise.all([ok<Info>(ctx, '/api/info'), ok<State>(ctx, '/api/state?limit=20000'), ok<GameFile>(ctx, '/api/game')])
      const byType: Record<string, number> = {}
      let players = 0
      for (const row of s.e) {
        const kind = row[4]
        if (kind === KIND_PLAYER) players++
        else if (kind === KIND_BOT || kind === KIND_DRIVEN) {
          const t = g.archetypes[row[3] ?? -1]?.name ?? '?'
          byType[t] = (byType[t] ?? 0) + 1
        }
      }
      const d: Daylight = g.daylight ?? DEFAULT_DAYLIGHT
      const p = phase(d, Date.now())
      return {
        admin: ctx.base(),
        world_size: info.world_size,
        seed: info.seed,
        players,
        bots: byType,
        load: { tick_avg_ms: s.perf.tick_avg_ms, tick_p99_ms: s.perf.tick_p99_ms, budget_ms: 1000 / s.hz },
        time_of_day: describe(d, p),
        weather: s.director?.weather ?? null,
        weather_zones: s.director?.zones ?? [],
        running_events: s.director?.runs ?? [],
        zone: s.zone ? { this: s.zone.names[s.zone.index] ?? s.zone.index, all: s.zone.names, mode: s.zone.lines ? 'lines' : 'strips' } : null,
        blocks: s.blocks?.total ?? 0,
        ground_items: s.game.ground_items ?? 0,
        game: {
          places: g.places.map((p) => `${p.name} (${p.x}, ${p.y}) r${p.r}${p.title ? ` "${p.title}"` : ''}${p.exit ? ` exit to ${p.exit.zone || 'here'} (${p.exit.x}, ${p.exit.y})` : ''}`),
          routes: g.routes.map((r) => r.name),
          bot_types: g.archetypes.map((a) => `${a.name} x${a.count}`),
          items: (g.items ?? []).map((i) => i.name),
          weather: (g.weather ?? []).map((w) => w.name),
          weather_zones: (g.zones ?? []).map((z) => z.name),
          events: (g.events ?? []).map((e) => e.name),
          attributes: g.attrs.map((a) => a.name),
          actions: g.actions.map((a) => a.name),
        },
      }
    },
  },
  {
    name: 'game_schema',
    description:
      'The TypeScript types of the game file (places, routes, bot types and their rules, items, weather, weather zones, events and their steps, attributes, actions, daylight), with comments. Read this before writing entries with upsert.',
    input: {},
    readOnly: true,
    async run() {
      const end = apiTypes.indexOf('export interface Perf')
      return { types: apiTypes.slice(0, end < 0 ? undefined : end).trim(), block_materials: MATERIALS }
    },
  },
  {
    name: 'get_game',
    description: 'The live game file, whole or one section.',
    input: { section: z.enum([...SECTION_NAMES, 'walls', 'daylight', 'settings']).optional() },
    readOnly: true,
    async run({ section }, ctx) {
      const g = await ok<GameFile>(ctx, '/api/game')
      if (!section) return g
      if (section === 'settings') return { move_speed: g.move_speed, road_speed: g.road_speed ?? 1.5, entity_radius: g.entity_radius, interest_radius: g.interest_radius, max_visible: g.max_visible }
      if (section === 'walls') return g.walls ?? 'generated from the world seed'
      if (section === 'daylight') return g.daylight ?? DEFAULT_DAYLIGHT
      return SECTIONS[section as Section].list(g)
    },
  },
  {
    name: 'upsert',
    description:
      'Add or change one entry of the game file, applied live: a place (a location players see named, an exit, or a sound zone: sound one of sea, river, forest, night, wind, rain, fire, town, market, crypt, holy, battle, and music one of explore, town, night, battle, eerie, holy, none), a route, a bot type (archetypes), an item, a weather, a weather zone, an event, an attribute or an action. Entries are matched by name. By default the fields given are merged over the existing entry (or over sensible defaults for a new one), so { "name": "camp", "r": 30 } only changes the radius. See game_schema for every field.',
    input: {
      section: z.enum(SECTION_NAMES),
      entry: z.record(z.string(), z.any()).describe('The entry, with at least its name.'),
      replace: z.boolean().optional().describe('Replace the whole entry instead of merging into it.'),
      allow_restart: allowRestart,
    },
    async run({ section, entry, replace, allow_restart }, ctx) {
      const s = section as Section
      const name = typeof entry.name === 'string' ? entry.name.trim() : ''
      if (!name) refuse('The entry needs a name.')
      let created = false
      const { applied, game } = await edit(
        ctx,
        (g) => {
          const ops = SECTIONS[s]
          const list = ops.list(g)
          const i = list.findIndex((e) => e.name === name)
          created = i < 0
          const merged = { ...(replace ? {} : i >= 0 ? list[i] : ops.fresh(g)), ...entry, name }
          return ops.put(g, i >= 0 ? list.map((e, k) => (k === i ? merged : e)) : [...list, merged])
        },
        allow_restart,
      )
      return { ...outcome(applied), [created ? 'added' : 'changed']: SECTIONS[s].list(game).find((e) => e.name === name) }
    },
  },
  {
    name: 'rename',
    description: 'Rename an entry of the game file. Everything that refers to it (bot rules, homes, event steps, weather zones, drops) follows the new name.',
    input: { section: z.enum(SECTION_NAMES), from: z.string(), to: z.string(), allow_restart: allowRestart },
    async run({ section, from, to, allow_restart }, ctx) {
      const s = section as Section
      const name = to.trim()
      if (!name) refuse('The new name is empty.')
      const { applied } = await edit(
        ctx,
        (g) => {
          if (SECTIONS[s].list(g).some((e) => e.name === name)) refuse(`There is already a ${s} entry called "${name}".`)
          return SECTIONS[s].rename(g, indexOf(g, s, from), name)
        },
        allow_restart,
      )
      return { ...outcome(applied), renamed: { from, to: name } }
    },
  },
  {
    name: 'remove',
    description:
      'Remove an entry of the game file. What referred to it goes with it, as in the console: bot rules that used a place or route, event steps that named it, a home. Removing a bot type takes its bots out of the world.',
    input: { section: z.enum(SECTION_NAMES), name: z.string(), allow_restart: allowRestart },
    destructive: true,
    async run({ section, name, allow_restart }, ctx) {
      const s = section as Section
      const { applied } = await edit(ctx, (g) => SECTIONS[s].remove(g, indexOf(g, s, name)), allow_restart)
      return { ...outcome(applied), removed: name }
    },
  },
  {
    name: 'add_preset',
    description: `Add a ready-made starting point, then adjust it with upsert. Bot types: ${BOT_TEMPLATES.map((t) => `${t.id} (${t.label})`).join('; ')}. Weather: ${WEATHER_PRESETS.map((t) => `${t.id} (${t.label})`).join('; ')}. Events: ${EVENT_PRESETS.map((t) => `${t.id} (${t.label})`).join('; ')}.`,
    input: {
      kind: z.enum(['bot', 'weather', 'event']),
      preset: z.string(),
      name: z.string().optional().describe('Name it this instead of the preset name.'),
    },
    async run({ kind, preset, name }, ctx) {
      const known = kind === 'bot' ? BOT_TEMPLATES : kind === 'weather' ? WEATHER_PRESETS : EVENT_PRESETS
      if (!known.some((p) => p.id === preset)) refuse(`No ${kind} preset "${preset}". Presets: ${known.map((p) => p.id).join(', ')}.`)
      const s: Section = kind === 'bot' ? 'archetypes' : kind === 'weather' ? 'weather' : 'events'
      let added = ''
      const { applied, game } = await edit(ctx, (g) => {
        const before = SECTIONS[s].list(g).length
        let next = kind === 'bot' ? addFromTemplate(g, preset) : kind === 'weather' ? addWeather(g, preset) : addEvent(g, preset)
        const list = SECTIONS[s].list(next)
        if (list.length === before) refuse(`There is no room for another ${kind}.`)
        added = list[list.length - 1]!.name
        if (name && name !== added) {
          if (list.some((e) => e.name === name)) refuse(`There is already a ${s} entry called "${name}".`)
          next = SECTIONS[s].rename(next, list.length - 1, name)
          added = name
        }
        return next
      })
      return { ...outcome(applied), added: SECTIONS[s].list(game).find((e) => e.name === added) }
    },
  },
  {
    name: 'set_settings',
    description:
      'Movement and visibility settings (road_speed: how much faster on roads), and the daylight schedule. Daylight applies live; the movement settings restart the world. Daylight is one of { mode: "cycle", day_minutes, night_share (0 to 0.9), offset }, { mode: "clock", sunrise: "HH:MM", sunset: "HH:MM", utc_offset_minutes }, or { mode: "fixed", phase (0 sunrise ... 1) }.',
    input: {
      move_speed: num.optional(),
      road_speed: num.min(1).max(4).optional().describe('How much faster everyone moves on a road (1 = no faster; default 1.5).'),
      entity_radius: num.optional(),
      interest_radius: num.optional(),
      max_visible: num.optional(),
      daylight: z.record(z.string(), z.any()).optional(),
      allow_restart: allowRestart,
    },
    async run({ allow_restart, daylight, ...rest }, ctx) {
      const set = Object.fromEntries(Object.entries(rest).filter(([, v]) => v !== undefined))
      const { applied, game } = await edit(ctx, (g) => ({ ...g, ...set, ...(daylight ? { daylight: daylight as Daylight } : {}) }), allow_restart)
      return { ...outcome(applied), settings: { move_speed: game.move_speed, road_speed: game.road_speed ?? 1.5, entity_radius: game.entity_radius, interest_radius: game.interest_radius, max_visible: game.max_visible }, daylight: game.daylight ?? DEFAULT_DAYLIGHT }
    },
  },
  {
    name: 'walls',
    description:
      'Hand-placed walls (flat boxes [x0, y0, x1, y1] that block movement). By default walls come from the world. add puts walls in (taking the generated ones over first), remove_in_area takes out those touching a circle or place, use_generated goes back to the world\'s own.',
    input: {
      op: z.enum(['add', 'remove_in_area', 'use_generated']),
      walls: z.array(z.tuple([num, num, num, num])).optional().describe('For add: [x0, y0, x1, y1] each.'),
      ...area,
    },
    async run({ op, walls, place, x, y, r }, ctx) {
      if (op === 'use_generated') return outcome((await edit(ctx, generatedWalls)).applied)
      if (op === 'add') {
        if (!walls?.length) refuse('Give the walls to add.')
        const current = (await ok<{ boxes: [number, number, number, number][] }>(ctx, '/api/world')).boxes
        const { applied, game } = await edit(ctx, (g) => ({ ...g, walls: [...(g.walls ?? current), ...walls] }))
        return { ...outcome(applied), walls: game.walls?.length }
      }
      const c = place ? await placeArea(ctx, place) : x !== undefined && y !== undefined && r !== undefined ? { x, y, r } : refuse('Give a place, or x, y and r.')
      const touches = (w: number[]) => {
        const nx = Math.max(w[0]!, Math.min(c.x, w[2]!))
        const ny = Math.max(w[1]!, Math.min(c.y, w[3]!))
        return dist(nx, ny, c.x, c.y) <= c.r
      }
      const current = (await ok<{ boxes: [number, number, number, number][] }>(ctx, '/api/world')).boxes
      let removed = 0
      const { applied } = await edit(ctx, (g) => {
        const all = g.walls ?? current
        const kept = all.filter((w) => !touches(w))
        removed = all.length - kept.length
        if (!removed) refuse('No walls there.')
        return { ...g, walls: kept }
      })
      return { ...outcome(applied), removed }
    },
  },
  {
    name: 'put_game',
    description: 'Replace the whole game file (as from get_game). Prefer upsert, rename and remove, which keep references right.',
    input: { game: z.record(z.string(), z.any()), allow_restart: allowRestart },
    destructive: true,
    async run({ game, allow_restart }, ctx) {
      return outcome((await edit(ctx, () => game as GameFile, allow_restart)).applied)
    },
  },

  // ------------------------------------------------------------ the world right now
  {
    name: 'list_entities',
    description: 'Who and what is in the world: players and bots with position and attributes. Filter by kind, bot type, or a circle.',
    input: {
      kind: z.enum(['player', 'bot', 'all']).optional(),
      type: z.string().optional().describe('Bot type name.'),
      ...area,
      limit: z.number().int().min(1).max(1000).optional().describe('Default 50.'),
    },
    readOnly: true,
    async run({ kind, type, place, x, y, r, limit }, ctx) {
      const [s, g] = await Promise.all([ok<State>(ctx, '/api/state?limit=20000'), ok<GameFile>(ctx, '/api/game')])
      const c = place ? await placeArea(ctx, place) : x !== undefined && y !== undefined && r !== undefined ? { x, y, r } : null
      const all = s.e
        .map((row) => entity(row, s, g))
        .filter((e) => e.kind !== 'ghost')
        .filter((e) => !kind || kind === 'all' || (kind === 'bot' ? e.kind === 'bot' || e.kind === 'driven' : e.kind === kind))
        .filter((e) => !type || e.type === type)
        .filter((e) => !c || dist(e.x, e.y, c.x, c.y) <= c.r)
      return { total: all.length, shown: all.slice(0, limit ?? 50) }
    },
  },
  {
    name: 'get_entity',
    description: 'One player or bot: position, attributes and what is in their bag.',
    input: { id: z.number().int() },
    readOnly: true,
    async run({ id }, ctx) {
      const [s, g] = await Promise.all([ok<State>(ctx, '/api/state?limit=20000'), ok<GameFile>(ctx, '/api/game')])
      const row = s.e.find((row) => row[0] === id)
      if (!row) refuse(`Nobody with id ${id} is in the world.`)
      const bag = await ctx.call(`/api/inventory?id=${id}`)
      return { ...entity(row!, s, g), bag: bag.status < 400 ? bag.body.bag : null }
    },
  },
  {
    name: 'spawn',
    description: 'Put bots of a type into the world now: at a place, around a point, or wherever the type lives.',
    input: {
      type: z.string().describe('Bot type name.'),
      count: z.number().int().min(1).max(5000).optional(),
      place: z.string().optional(),
      x: num.optional(),
      y: num.optional(),
      radius: num.optional().describe('Scatter within this distance of the point (default: the place radius).'),
    },
    async run({ type, count, place, x, y, radius }, ctx) {
      const at = place ? await placeArea(ctx, place) : x !== undefined && y !== undefined ? { x, y, r: radius ?? 0 } : null
      return ok(ctx, '/api/spawn', 'POST', { archetype: type, count: count ?? 1, ...(at ? { x: at.x, y: at.y, radius: radius ?? at.r } : {}) })
    },
  },
  {
    name: 'remove_entities',
    description: 'Take bots out of the world: one by id, every bot of a type, or every bot. Players cannot be removed.',
    input: { id: z.number().int().optional(), type: z.string().optional(), all_bots: z.boolean().optional() },
    destructive: true,
    async run({ id, type, all_bots }, ctx) {
      if (id !== undefined) return ok(ctx, '/api/kill', 'POST', { id })
      if (all_bots) return ok(ctx, '/api/kill', 'POST', { all_bots: true })
      if (!type) refuse('Give an id, a type, or all_bots.')
      const [s, g] = await Promise.all([ok<State>(ctx, '/api/state?limit=20000'), ok<GameFile>(ctx, '/api/game')])
      const a = g.archetypes.findIndex((a: Archetype) => a.name === type)
      if (a < 0) refuse(`There is no bot type called "${type}".`)
      const ids = s.e.filter((row) => row[3] === a && row[4] === KIND_BOT).map((row) => row[0]!)
      let killed = 0
      for (const id of ids) killed += (await ctx.call('/api/kill', 'POST', { id })).body?.killed ?? 0
      return { ok: true, killed }
    },
  },
  {
    name: 'drive',
    description: 'Take control of a bot: move it by (dx, dy) and optionally perform an action on the nearest thing in range. It stays driven until release.',
    input: { id: z.number().int(), dx: num.optional(), dy: num.optional(), action: z.string().optional() },
    async run({ id, dx, dy, action }, ctx) {
      return ok(ctx, '/api/drive', 'POST', { id, dx: dx ?? 0, dy: dy ?? 0, ...(action ? { action } : {}) })
    },
  },
  {
    name: 'release',
    description: 'Hand a driven bot back to its own rules.',
    input: { id: z.number().int() },
    async run({ id }, ctx) {
      return ok(ctx, '/api/release', 'POST', { id })
    },
  },
  {
    name: 'say',
    description: 'Make a bot say something in chat (nearby, or shouted to everyone). For a message to every player, use an event with an announce step.',
    input: { id: z.number().int(), text: z.string(), shout: z.boolean().optional() },
    async run({ id, text, shout }, ctx) {
      return ok(ctx, '/api/say', 'POST', { id, text, shout: !!shout })
    },
  },
  {
    name: 'read_chat',
    description: 'Recent chat, newest last: who said what, and banners.',
    input: { limit: z.number().int().min(1).max(500).optional() },
    readOnly: true,
    async run({ limit }, ctx) {
      const log = await ok<{ lines: [number, number, number, string, string?][] }>(ctx, '/api/chat')
      const ch = ['say', 'shout', 'name', 'banner']
      return log.lines.slice(-(limit ?? 50)).map(([tick, from, c, text, name]) => ({ tick, from: name ?? (from === 0 ? 'server' : from), channel: ch[c] ?? c, text }))
    },
  },
  {
    name: 'god_mode',
    description: 'Make a player unhurtable (or not).',
    input: { id: z.number().int(), on: z.boolean() },
    async run({ id, on }, ctx) {
      return ok(ctx, '/api/god', 'POST', { id, on })
    },
  },
  {
    name: 'give_item',
    description: 'Put items in a player\'s bag (what does not fit drops at their feet).',
    input: { id: z.number().int(), item: z.string(), count: z.number().int().min(1).max(255).optional() },
    async run({ id, item, count }, ctx) {
      return ok(ctx, '/api/give', 'POST', { id, item, count: count ?? 1 })
    },
  },
  {
    name: 'drop_items',
    description: 'Leave items on the ground for players to pick up: at a place (scattered over it) or a point.',
    input: { item: z.string(), count: z.number().int().min(1).max(500).optional(), place: z.string().optional(), x: num.optional(), y: num.optional() },
    async run({ item, count, place, x, y }, ctx) {
      return ok(ctx, '/api/ground', 'POST', { item, count: count ?? 1, ...(place ? { place } : {}), ...(x !== undefined && y !== undefined ? { x, y } : {}) })
    },
  },
  {
    name: 'clear_ground_items',
    description: 'Take away items lying on the ground: in a place, in a circle, or everywhere (everything: true).',
    input: { ...area, everything: z.boolean().optional() },
    destructive: true,
    async run(args, ctx) {
      return ok(ctx, '/api/ground/clear', 'POST', clearBody(args))
    },
  },

  // ------------------------------------------------------------ sky and events, now
  {
    name: 'set_weather',
    description: 'Change the weather everywhere now, for a while or until changed (name null clears it).',
    input: { name: z.string().nullable(), duration_secs: z.number().int().min(0).max(86400).optional().describe('0 or absent: until changed.') },
    async run({ name, duration_secs }, ctx) {
      return ok(ctx, '/api/weather', 'POST', { name, duration_secs: duration_secs ?? 0 })
    },
  },
  {
    name: 'switch_weather_zone',
    description: 'Turn a weather zone (weather that stays over one place) on or off now.',
    input: { name: z.string(), on: z.boolean() },
    async run({ name, on }, ctx) {
      return ok(ctx, '/api/zone', 'POST', { name, on })
    },
  },
  {
    name: 'set_time',
    description: 'Move the time of day now: dawn, noon, dusk or midnight, or a phase (0 sunrise, rising through the day to 1).',
    input: { at: z.enum(['dawn', 'noon', 'dusk', 'midnight']).optional(), phase: num.min(0).max(1).optional() },
    async run({ at, phase: p }, ctx) {
      const g = await ok<GameFile>(ctx, '/api/game')
      const d = g.daylight ?? DEFAULT_DAYLIGHT
      const to = at ? moment(d, at) : p ?? refuse('Give at or phase.')
      const r = await ok(ctx, '/api/daylight', 'POST', { phase: to })
      return { ...r, now: describe(d, to) }
    },
  },
  {
    name: 'event',
    description: 'Start an event now, or stop one that is running (its bots leave).',
    input: { name: z.string(), op: z.enum(['start', 'stop']) },
    async run({ name, op }, ctx) {
      return ok(ctx, op === 'start' ? '/api/events/fire' : '/api/events/stop', 'POST', { name })
    },
  },
  {
    name: 'director_status',
    description: 'Weather, weather zones, each event (times started, when next, how it last ended), running events step by step, and the recent director log.',
    input: {},
    readOnly: true,
    async run(_, ctx) {
      const s = await ok<State>(ctx, '/api/state?limit=0')
      return s.director ?? null
    },
  },

  // ------------------------------------------------------------ building
  {
    name: 'build',
    description: `Place or clear blocks (${BLOCK}x${BLOCK} world units each, ${LAYERS} layers stacked from the ground up). Shapes over a rectangle or along a line in world coordinates: fill (solid), walls (the outline only: a building's walls), line (a wall from one point to another). Layers bottom .. bottom + height - 1 are set to the material, keeping what else is in each column; material "air" clears them. Materials: ${MATERIALS.join(', ')} (lamp and fire glow at night). A new column is refused where someone stands.`,
    input: {
      shape: z.enum(['fill', 'walls', 'line']),
      x0: num,
      y0: num,
      x1: num,
      y1: num,
      material: z.union([z.string(), z.number().int()]).describe('A material name or number, or "air" to clear.'),
      bottom: z.number().int().min(0).max(LAYERS - 1).optional().describe('First layer, 0 at the ground (default 0).'),
      height: z.number().int().min(1).max(LAYERS).optional().describe('Layers (default 1).'),
    },
    async run({ shape, x0, y0, x1, y1, material, bottom, height }, ctx) {
      const cells = shapeCells(shape, x0, y0, x1, y1)
      const ch = material === 'air' ? '.' : materialChar(material)
      const b = bottom ?? 0
      const h = Math.min(height ?? 1, LAYERS - b)
      const have = await ok<{ cols: [number, number, number, string, number[]][] }>(ctx, '/api/blocks')
      const now = new Map(have.cols.map((c) => [`${c[0]},${c[1]}`, c[3]]))
      const cols = cells.map(([cx, cy]) => [cx, cy, overlay(now.get(`${cx},${cy}`) ?? '', b, h, ch)])
      const r = await ok<{ set: number; refused: number; total: number }>(ctx, '/api/blocks', 'POST', { cols })
      return { ...r, cells: cells.length, layers: [b, b + h - 1] }
    },
  },
  {
    name: 'get_blocks',
    description: 'Built blocks in a circle or place: each column as [cx, cy, text], where cell (cx, cy) covers world x cx*2 .. cx*2+2 and the text has one character per layer from the ground (. empty, else the material number in base 36).',
    input: { ...area, limit: z.number().int().min(1).max(5000).optional() },
    readOnly: true,
    async run({ place, x, y, r, limit }, ctx) {
      const c = place ? await placeArea(ctx, place) : x !== undefined && y !== undefined && r !== undefined ? { x, y, r } : refuse('Give a place, or x, y and r.')
      const have = await ok<{ total: number; cols: [number, number, number, string, number[]][] }>(ctx, '/api/blocks')
      const near = have.cols.filter((k) => dist((k[0] + 0.5) * BLOCK, (k[1] + 0.5) * BLOCK, c.x, c.y) <= c.r)
      return { total_in_world: have.total, columns: near.length, shown: near.slice(0, limit ?? 500).map((k) => [k[0], k[1], k[3]]), materials: MATERIALS }
    },
  },
  {
    name: 'clear_blocks',
    description: 'Take away built blocks: in a place, in a circle, those built by one account (owner tag from the console), or every block in the world (everything: true; cannot be undone).',
    input: { ...area, owner: z.string().optional(), everything: z.boolean().optional() },
    destructive: true,
    async run({ owner, ...args }, ctx) {
      return ok(ctx, '/api/blocks/clear', 'POST', { ...clearBody(args, owner !== undefined), ...(owner !== undefined ? { owner } : {}) })
    },
  },

  // ------------------------------------------------------------ the land
  {
    name: 'get_world',
    description:
      'The land: size, the recipe it was generated from (city, villages, forts, crags, lakes, rivers, roads, forest, hills, hand edits), counts of what is on it and how many roads, and the objects (houses, keeps, towers, walls, trees...) in a circle or place, or of chosen types anywhere. Villages are a square (the green) on a high street lined with houses; forts are keeps.',
    input: {
      ...area,
      types: z.array(z.string()).optional().describe('Only objects of these types (for example ["city", "castle", "keep", "square", "lake", "crag"]): over the whole world when no area is given.'),
      limit: z.number().int().min(1).max(2000).optional(),
    },
    readOnly: true,
    async run({ place, x, y, r, types, limit }, ctx) {
      const file = await ctx.call('/api/worldfile')
      if (file.status === 404) {
        const info = await ok<Info>(ctx, '/api/info')
        return { size: info.world_size, recipe: null, note: 'This server has no world file: its land and walls come from its seed. world_preview and world_apply make one.' }
      }
      if (file.status >= 400) refuse(file.body?.error ?? `HTTP ${file.status}`)
      const w = file.body
      const c = place ? await placeArea(ctx, place) : x !== undefined && y !== undefined && r !== undefined ? { x, y, r } : null
      const objs: any[] = w.objects ?? []
      const want = types?.length ? new Set<string>(types) : null
      const near = c || want
        ? objs.filter((o) => {
            if (want && !want.has(o.t)) return false
            if (!c) return true
            const [ox, oy] = o.p ?? [(o.b[0] + o.b[2]) / 2, (o.b[1] + o.b[3]) / 2]
            return dist(ox, oy, c.x, c.y) <= c.r
          })
        : []
      return {
        size: w.size,
        recipe: w.recipe ?? null,
        counts: countBy(objs.map((o) => o.t)),
        rivers: (w.water?.rivers ?? w.rivers ?? []).length,
        lakes: (w.water?.lakes ?? w.lakes ?? []).length, roads: (w.roads ?? []).length,
        ...(c || want ? { objects_here: near.slice(0, limit ?? 200).map((o) => ({ t: o.t, ...(o.b ? { b: o.b } : { p: o.p }) })), objects_here_total: near.length } : {}),
      }
    },
  },
  {
    name: 'world_preview',
    description:
      'Generate land from a recipe without applying it: the fields given go over the current recipe (size, seed, city, villages, forts, crags, lakes, rivers, roads (true/false: a network of roads joining the city, villages and forts, which everyone moves faster on), forest, hills, coast ([[[x, y], ...]]: land inside the outlines, sea outside), river_courses ([[[x, y], ...]] source to mouth), settlements ([{ "t": city|town|village|fort, "at": [x, y], "name": "...", "side": city width }] placed where asked; settled, in the world file, says where each went), and edits: [{ "remove": [x, y] } | { "add": { "t": house|keep|tower|castle|wall|crag|lake|square|city, "b": [x0, y0, x1, y1] } }] applied in order). Returns what the land would hold; world_apply makes it real.',
    input: { recipe: z.record(z.string(), z.any()) },
    readOnly: true,
    async run({ recipe }, ctx) {
      const cur = await ctx.call('/api/worldfile')
      const info = await ok<Info>(ctx, '/api/info')
      const base = (cur.status < 400 && cur.body?.recipe) || defaultRecipe(info.world_size, info.seed)
      const full = { ...base, ...recipe }
      const w = await ok<any>(ctx, '/api/world/generate', 'POST', full)
      return { recipe: full, size: w.size, counts: countBy((w.objects ?? []).map((o: any) => o.t)), rivers: (w.water?.rivers ?? w.rivers ?? []).length, lakes: (w.water?.lakes ?? w.lakes ?? []).length, roads: (w.roads ?? []).length }
    },
  },
  {
    name: 'world_apply',
    description: 'Make the last world_preview the world, live: the land, its walls and its objects change for everyone. Built blocks stay.',
    input: {},
    destructive: true,
    async run(_, ctx) {
      return ok(ctx, '/api/world/apply', 'POST', {})
    },
  },
]

function countBy(xs: string[]): Record<string, number> {
  const out: Record<string, number> = {}
  for (const x of xs) out[x] = (out[x] ?? 0) + 1
  return out
}

/** A clear-down's filters. Nothing at all means the whole world, so that has to be asked for in so many words. */
function clearBody({ place, x, y, r, everything }: { place?: string; x?: number; y?: number; r?: number; everything?: boolean }, filtered = false) {
  if (place) return { place }
  if (x !== undefined || y !== undefined || r !== undefined) return { x, y, r }
  if (!everything && !filtered) refuse('Give a place, or x, y and r, or everything: true to clear the whole world.')
  return {}
}
