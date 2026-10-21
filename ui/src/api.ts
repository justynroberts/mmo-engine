import type { Daylight } from './daylight'
export type Behaviour = 'idle' | 'wander' | 'chase' | 'flee'
export type Who = 'self' | 'target'

export interface AttrFile {
  name: string
  min: number
  max: number
  default: number
  bits: 8 | 16
  replicated: boolean
  regen_per_sec: number
  on_zero: 'none' | 'respawn'
}
export interface EffectFile {
  to: Who
  attr: string
  delta: number
}
export interface ActionFile {
  id: number
  name: string
  cooldown_ticks: number
  range: number
  requires: { attr: string; min: number } | null
  effects: EffectFile[]
}
export type Seek = 'player' | 'bot' | 'any'
export type Cond =
  | { kind: 'always' }
  | { kind: 'sees'; who: Seek; within: number }
  | { kind: 'attr_below'; attr: string; value: number }
  | { kind: 'attr_above'; attr: string; value: number }
  | { kind: 'in_place'; place: string }
  | { kind: 'chance'; pct: number }
export type Act =
  | { kind: 'idle' | 'wander' | 'chase' | 'flee' | 'attack' }
  | { kind: 'patrol'; route: string }
  | { kind: 'goto'; place: string }
  | { kind: 'say'; text: string; shout: boolean }
export interface Rule {
  when: Cond
  then: Act
}
export interface Place {
  name: string
  x: number
  y: number
  r: number
  /** What players see as they walk in and out; empty or missing for a place that is not a location. */
  title?: string
  /** A zone line: walking in takes a player to (x, y) in `zone` (a zone's name), or on this map if `zone` is empty. */
  exit?: { zone?: string; x: number; y: number }
}
export interface Route {
  name: string
  points: [number, number][]
  loop: boolean
}

export interface UseEffect {
  attr: string
  delta: number
}
export interface ItemDef {
  name: string
  color: string
  stack_max: number
  uses: UseEffect[]
  consume: boolean
}
export interface DropDef {
  item: string
  chance_pct: number
  min: number
  max: number
}

export interface Archetype {
  name: string
  color: string
  count: number
  behaviour: Behaviour
  speed_mult: number
  sight: number
  attack: string | null
  attrs: Record<string, number>
  home: string | null
  rules: Rule[]
  drops?: DropDef[]
}
export interface Drain {
  attr: string
  per_sec: number
}
export interface WeatherDef {
  name: string
  color: string
  intensity: number
  sight_mult: number
  drain: Drain[]
}
export type Step =
  | { kind: 'announce'; text: string; banner?: boolean; nearby?: boolean }
  | { kind: 'weather'; name: string | null; duration_secs: number }
  | { kind: 'spawn'; archetype: string; count: number; place: string | null; name?: string | null }
  | { kind: 'hit'; attr: string; delta: number; place: string | null }
  | { kind: 'clear'; archetype: string }
  | { kind: 'drop'; item: string; count: number; place: string | null }
  | { kind: 'zone'; zone: string; on: boolean }
  | { kind: 'wait'; secs: number }
  | { kind: 'defeat'; within_secs: number }
  | { kind: 'reward'; item: string; count: number; everyone?: boolean }
export type DayPart = 'dawn' | 'day' | 'dusk' | 'night'
/** What starts an event on its own, besides its timer. */
export type When =
  | { kind: 'players'; count: number }
  | { kind: 'time'; at: DayPart }
  | { kind: 'after'; event: string; outcome?: 'success' | 'failure' | 'either' }
/** A weather that stays over one place: drawn as mist there, in effect only inside. */
export interface WeatherZone {
  name: string
  place: string
  weather: string
  active?: boolean
  height?: number
}
export interface EventDef {
  name: string
  every_secs: number
  chance_pct: number
  when?: When | null
  /** Least seconds between two starts by `when` (default 600). */
  cooldown_secs?: number
  /** Where it happens: a local event. */
  place?: string | null
  /** Longest it may run before it has failed; 0 for no limit. */
  limit_secs?: number
  steps: Step[]
  /** What happens instead if it fails. */
  fail_steps?: Step[]
}
/** An event in progress (`/api/state` director.runs). */
export interface RunRow {
  name: string
  step: number
  of: number
  failing: boolean
  waiting: string | null
  foes: number
  people: number
  secs: number
  left_secs: number | null
}
export interface DirectorState {
  weather: { name: string; left_secs: number | null } | null
  weathers: string[]
  /** Each weather zone and whether it is on right now. */
  zones?: { name: string; on: boolean }[]
  events: { name: string; fired: number; next_in_secs: number | null; last?: string | null }[]
  runs?: RunRow[]
  /** Newest first: [tick, text]. */
  log: [number, string][]
}

export interface GameFile {
  move_speed: number
  entity_radius: number
  interest_radius: number
  max_visible: number
  attrs: AttrFile[]
  actions: ActionFile[]
  archetypes: Archetype[]
  places: Place[]
  routes: Route[]
  /** Hand-placed walls [x0, y0, x1, y1]; absent means generated from the world seed. */
  walls?: [number, number, number, number][]
  items?: ItemDef[]
  weather?: WeatherDef[]
  events?: EventDef[]
  zones?: WeatherZone[]
  /** When it is light and dark (absent: the default cycle). */
  daylight?: Daylight
}

export interface Perf {
  tick_avg_ms: number
  tick_p99_ms: number
  recv_ms: number
  sim_ms: number
  snap_ms: number
  send_ms: number
  in_pps: number
  out_pps: number
  mbit: number
  lanes: number
}
export interface NetCounters {
  hellos: number
  connects: number
  inputs: number
  bad: number
  bad_cookie: number
  auth_fail: number
  rejected: number
  timeouts: number
  /** Players who signed in here but had last left through a zone line, so were sent on to that zone. */
  sent_on?: number
}
/** This server's place among the zones, when it runs as one (`--zone`). */
export interface ZoneState {
  index: number
  count: number
  /** Separate maps joined by exits (`--zone-lines`) rather than strips of one world. */
  lines: boolean
  /** Each zone's name, by index. */
  names: string[]
  lo: number
  hi: number
  /** Players handed to another zone, and taken from one. */
  out: number
  in: number
  failed: number
  rejected: number
  ghosts: number
}
/** Entity row: [id, x, y, archetype (-1 none), kind (0 bot, 1 player, 2 driven), ...attrs] */
export type EntityRow = number[]
export interface State {
  tick: number
  hz: number
  live: number
  clients: number
  hash: string
  perf: Perf
  net: NetCounters
  zone?: ZoneState
  game: { zaps: number; respawns: number; rejected_actions: number; ground_items?: number }
  attrs: string[]
  e: EntityRow[]
  shown: number
  /** Players' display names by entity id (bots go by their type). */
  names?: Record<string, string>
  director?: DirectorState
  /** Blocks built by players: `ver` changes whenever they do (refetch /api/blocks). */
  blocks?: { ver: number; total: number }
  /** The world file's version, when the server has one (refetch /api/worldfile when it changes). */
  world_ver?: string
}
export interface Info {
  server_key: string
  content_hash: string
  hz: number
  udp_port: number
  world_size: number
  seed: number
  proto: number
  capacity: number
  snapshot_lanes: number
  actions: { id: number; name: string; range: number }[]
}
export interface WorldGeom {
  size: number
  boxes: [number, number, number, number][]
}
export interface PutResult {
  ok: boolean
  error?: string
  reset?: boolean
  dropped_clients?: number
  moved_entities?: number
  content_hash?: string
  saved?: boolean | string
}

export interface Reply<T> {
  status: number
  body: T
}

export async function api<T>(path: string, method = 'GET', body?: unknown, signal?: AbortSignal): Promise<Reply<T>> {
  const res = await fetch(path, {
    method,
    signal,
    headers: method === 'GET' ? undefined : { 'X-Admin': '1', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body),
  })
  const text = await res.text()
  let parsed: unknown = null
  try {
    parsed = JSON.parse(text)
  } catch {
    parsed = { error: text.slice(0, 200) }
  }
  return { status: res.status, body: parsed as T }
}

export const KIND_BOT = 0
export const KIND_PLAYER = 1
export const KIND_DRIVEN = 2
/** A mirror of an entity in a neighbouring zone (inert). */
export const KIND_GHOST = 3

export interface ChatLog {
  tick: number
  /** [tick, speaker id, channel (0 say, 1 shout), text, the speaker's name now ("" if gone)] */
  lines: [number, number, number, string, string?][]
}
